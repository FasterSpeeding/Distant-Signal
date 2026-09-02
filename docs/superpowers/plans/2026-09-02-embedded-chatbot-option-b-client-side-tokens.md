# Embedded Chatbot Option B: Client-Side Anthropic Keys Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the client-side-token redesign of embedded-chatbot Option B —
every Anthropic API call made directly from the user's own browser with a
key the user supplies (never seen by any Distant Signal server), and the
browser authenticating to `distant-signal-mcp` via the same interactive
OAuth flow Option C's Claude Desktop already uses — **on top of** the real,
already-implemented but now-stale Option B branches in both repositories,
by first reconciling those branches against each repo's current default
branch (real conflicts, not a formality), then removing the DS-hosted
`orchestrator/` service and the `urn:distant-signal:orchestrator-session`
grant it depended on, adding CORS to `distant-signal-mcp`'s `/mcp`, and
building the browser-side OAuth client + Anthropic key storage + relocated
tool-calling loop.

**Architecture:**

```
Task 1 (this repo): reconcile worktree-agent-a6fc94940b8aa651c onto main
Task 2 (distant-signal-mcp): reconcile feat/embedded-chatbot-option-b onto master
        │ both branches now current, all pre-existing behavior intact
        ▼
Task 3 (distant-signal-mcp): add CORS to /mcp, scoped to config.frontendOrigin
Task 4 (distant-signal-mcp): remove the orchestrator-session grant + 12 tests
        │
        ▼
Task 5 (this repo): remove orchestrator/ entirely -- service, chart, CI,
                     compose, env examples, its 18 tests, the now-obsolete
                     same-origin SSE proxy (frontend/app/api/chat/route.ts)
Task 6 (this repo): repurpose chatbot_allowed_users' doc comments (Decision 4)
        │
        ▼
Task 7 (this repo): frontend deps + BrowserMcpOAuthProvider (localStorage)
Task 8 (this repo): frontend/app/chat/callback -- code exchange, NEW route
Task 9 (this repo): Anthropic key entry/storage UI + one-time disclosure
Task 10 (this repo): ChatPanel.tsx rewrite -- browser tool-calling loop,
                      three distinct error states
Task 11 (this repo): Playwright e2e coverage for /chat (mocked network)
Task 12 (both repos): final cross-repo verification
```

**Tech Stack:** Rust (`crates/api`, `sqlx`) for Task 1's conflict and Task 6.
Helm (`charts/distant-signal/`) + `.github/workflows/containers.yml` +
`docker-compose.yml` for Task 5's removal. TypeScript/Express
(`distant-signal-mcp`, its own separate repository) for Tasks 2-4 — `cors`
(new direct dependency), the already-installed `@modelcontextprotocol/sdk`
`1.29.0`. Next.js App Router + TypeScript (`frontend/`) for Tasks 7-11 —
new dependencies `@modelcontextprotocol/sdk` (`^1.29.0`) and
`@anthropic-ai/sdk` (`^0.123.0`, matching the version `orchestrator/`'s own
now-deleted `package.json` pinned), Vitest (unit), Playwright (e2e, already
configured).

**Spec:** `docs/superpowers/specs/2026-09-02-embedded-chatbot-option-b-client-side-tokens-design.md`
— read in full before starting; every Decision (1-7) below maps to a task.
Also required reading: `docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b.md`
(the plan whose 7 tasks are already implemented, unmerged, on the two
branches Tasks 1-2 reconcile) and
`docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md`
(background — largely superseded by the client-side-tokens design per its
own Corrections section, cited only for context).

## Coordination note — re-verified this session, both repos' real conflicts identified precisely

**This plan's own re-verification (2026-09-02, against the actual current
`main`/`master` — not the design spec's own, now slightly older, snapshot)
found FEWER real conflicts than a first glance at "both branches touch
files that also changed on `main`" would suggest.** `git merge-tree` against
each repo's actual merge-base was run for both branches; only files with
literal `<<<<<<<` conflict markers in that output require hand resolution —
every other file `git merge-tree` reported as "changed in both" merges
clean automatically (both sides' hunks land in non-overlapping regions of
the same file). Task 1 and Task 2 below give the **exact** commit each
conflict surfaces on, the **exact** resolved content, and which files that
"changed in both" but need zero manual intervention.

**In this repo (`worktree-agent-a6fc94940b8aa651c` vs `main`,
merge-base `6fc52c0`, `main` now 31 commits ahead):** exactly two files
conflict — `crates/api/src/data/users.rs` (an adjacent-test-append
conflict against the `users.groups`/internal-service-OAuth2 rewrite's own
new test) and `charts/distant-signal/values.yaml` (the
`internal-service-oauth2`/`mcp-server-oauth-access-groups` work's own new
`railMcp.accessGroups` block landing in the same spot as this branch's own
new `railMcp.orchestratorInternalToken` block). `crates/api/src/auth.rs`,
`crates/api/src/routes/mod.rs`, `charts/distant-signal/templates/_helpers.tpl`,
`charts/distant-signal/templates/secret.yaml`, `docker-compose.yml`,
`dev.env.example`, `local.env.example` all show as "changed in both" but
merge automatically clean — do not spend time pre-emptively hand-resolving
these, `git rebase`'s own automatic merge handles them.

**In `distant-signal-mcp` (`feat/embedded-chatbot-option-b` vs `master`,
merge-base `52b637d`, `master` now 8 commits ahead — the
`mcp-server-oauth-access-groups` and CI-workflow work):** exactly two files
conflict — `src/config.ts` (the `oauth.orchestratorInternalToken` field
landing inside the same object-literal region as `master`'s own new
`accessGroups` sibling field) and `test/config.test.ts` (an adjacent-test-append
conflict, same shape as the `users.rs` one above). `src/app.ts` and
`test/app.test.ts` both show as "changed in both" but merge automatically
clean.

## Global Constraints

- **Tasks 2-4 live in `distant-signal-mcp`, a completely separate git
  repository from this one, checked out at `/workspaces/distant-signal-mcp`
  in this environment (not inside this worktree's isolation).** Every step
  in those tasks must be run from that repository's own working copy on
  its own feature branch — never from this worktree. That repository has
  no configured remote in this environment (`git remote -v` returns
  nothing); an implementer working against a real fork should push to
  `origin` and open a PR there as the final step, adjusting the exact
  remote/PR commands below to whatever remote actually exists in their
  environment.
- **Do not merge this plan's own work to `main` (this repo) or `master`
  (`distant-signal-mcp`) as part of executing it** — land it on a feature
  branch in each repository; merging both is a separate, later decision
  the repo owner makes once this plan's tasks are verified.
- **Every removal task (5, 4) is a genuine deletion of real, already-
  passing test coverage** — Task 5 deletes 18 `orchestrator/` tests and 6
  `frontend/app/api/chat/route.test.ts` tests; Task 4 deletes 12
  `oauth-orchestrator-grant.test.ts` tests. Do not attempt to "adapt" or
  port these tests to the new architecture — the code they tested no
  longer exists after these tasks; new coverage for the browser-side
  replacement is added by Tasks 7-11, not by preserving these.
- **`frontend/app/chat/page.tsx` and `frontend/lib/api.ts`'s
  `getChatbotAccess()` are UNCHANGED by this entire plan.** Only
  `ChatPanel.tsx` (the client component `page.tsx` renders) is rewritten
  (Task 10). `page.test.tsx`'s existing 3 tests keep passing throughout —
  if any task's diff touches `page.tsx` or breaks those tests, that is a
  sign of scope creep, not a required change.
- **Anthropic's own CORS support for `dangerouslyAllowBrowser: true`
  needs zero code changes on either repository** (design doc Decision 2) —
  no task in this plan touches anything Anthropic-API-facing beyond
  passing that flag when constructing the client (Task 10).
- **Verification commands used throughout:** this repo —
  `cargo test -p api` (non-`#[ignore]`d tests run without a database;
  `#[ignore]`d ones need `DATABASE_URL` set against a live Postgres, same
  incantation prior plans in this repo's `docs/superpowers/plans/` use),
  `cd frontend && npm test`, `cd frontend && npx tsc --noEmit`,
  `helm template charts/distant-signal --set railMcp.enabled=true --set railMcp.publicUrl=https://example.com --set railMcp.frontendOrigin=https://example.com > /dev/null`.
  `distant-signal-mcp` — `npm test`, `npm run typecheck`.

---

## Task 1: Reconcile the main-repo Option B branch onto current `main`

**Files:**
- Modify (during rebase, conflict resolution only): `crates/api/src/data/users.rs`, `charts/distant-signal/values.yaml`
- No new files

**Interfaces:**
- Produces: a branch (`embedded-chatbot-option-b-client-side-tokens`,
  created from `worktree-agent-a6fc94940b8aa651c`, rebased onto current
  `main`) carrying all 5 of that branch's original commits, conflict-free,
  with `main`'s 31 intervening commits (the `users.groups`/internal-service-
  OAuth2 rewrite and the `mcp-server-oauth-access-groups` chart/CI work)
  fully present and untouched.
- **Depends on:** nothing. This is the prerequisite every other task in
  this repo builds on.

The branch `worktree-agent-a6fc94940b8aa651c` carries 5 commits not on
`main`: `098e6f6` (chatbot allowlist), `4cae0b8` (orchestrator service),
`940cc8e` (SSE proxy), `c8e0178` (chat UI), `92c7576` (chart+CI). Rebasing
them onto `main` stops exactly twice — once on `098e6f6`, once on
`92c7576` — per this session's own `git merge-tree` re-verification
(Coordination note above).

- [ ] **Step 1: Start the rebase**

```bash
git checkout worktree-agent-a6fc94940b8aa651c
git checkout -b embedded-chatbot-option-b-client-side-tokens
git rebase main
```

- [ ] **Step 2: Resolve the first conflict — `crates/api/src/data/users.rs`, surfaces on commit `098e6f6`**

`git rebase` will report a conflict inside the `#[cfg(test)] mod db_tests`
block. `main`'s own `groups_are_overwritten_not_merged_on_repeat_login`
test (from the concurrent `users.groups` work) and this branch's own
`is_chatbot_allowed_reflects_allowlist_membership` test were both appended
at the same point in the file — **keep both tests in full, back to back**,
neither one replaces or subsumes the other (they test unrelated things:
OIDC group persistence vs. chatbot allowlist membership). The
non-test `is_chatbot_allowed` function itself (added just above the test
module) does not conflict — only the two tests do. Resolve the marked
region to:

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                groups_are_overwritten_not_merged_on_repeat_login -- --ignored`"]
    async fn groups_are_overwritten_not_merged_on_repeat_login() {
        use sqlx::postgres::PgPoolOptions;

        let database_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let mut identity = OidcIdentity {
            sub: "TEST-USER-GROUPS-OVERWRITE".to_string(),
            email: Some("test@example.com".to_string()),
            // ... (unchanged from `main`'s own version of this test --
            // do not alter its body, only its position relative to the
            // test below)
        };
        // ... rest of this test's body, verbatim from `main`
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                is_chatbot_allowed -- --ignored`"]
    async fn is_chatbot_allowed_reflects_allowlist_membership() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ('TEST-USER-CHATBOT-ALLOWED', NULL, NULL) \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user");

        assert!(
            !is_chatbot_allowed(&pool, "TEST-USER-CHATBOT-ALLOWED")
                .await
                .expect("lookup before allowlisting"),
            "not allowlisted yet"
        );

        sqlx::query(
            "INSERT INTO chatbot_allowed_users (user_id) VALUES ('TEST-USER-CHATBOT-ALLOWED')",
        )
        .execute(&pool)
        .await
        .expect("insert allowlist row");
        assert!(
            is_chatbot_allowed(&pool, "TEST-USER-CHATBOT-ALLOWED")
                .await
                .expect("lookup after allowlisting"),
            "allowlisted now"
        );

        // A user_id with no `users` row at all -- same false as a resolvable
        // but non-allowlisted user, no separate error case.
        assert!(
            !is_chatbot_allowed(&pool, "TEST-USER-DOES-NOT-EXIST")
                .await
                .expect("lookup for a nonexistent user")
        );

        // Cleanup -- chatbot_allowed_users cascades via ON DELETE CASCADE.
        sqlx::query("DELETE FROM users WHERE id = 'TEST-USER-CHATBOT-ALLOWED'")
            .execute(&pool)
            .await
            .expect("cleanup test user");
    }
```

(The `groups_are_overwritten_not_merged_on_repeat_login` body above is
elided with a comment because it belongs to `main`'s own commit, not this
plan — copy it verbatim from `main`'s version of the file rather than
retyping it; only its *position*, ahead of the chatbot test, is what this
step decides.)

```bash
git add crates/api/src/data/users.rs
git rebase --continue
```

- [ ] **Step 3: Resolve the second conflict — `charts/distant-signal/values.yaml`, surfaces on commit `92c7576`**

The conflict is inside `railMcp:`, right after
`existingSecretInternalCompleteTokenKey: internal-complete-token`. `main`'s
own `accessGroups` block (from `mcp-server-oauth-access-groups`, already
landed) and this branch's own `orchestratorInternalToken` block both land
at that exact point. **Keep both** — this task only reconciles, it does
not yet remove anything (Task 5 removes `orchestratorInternalToken` once
`orchestrator/` itself is deleted). Resolve to:

```yaml
  # -- Authentik-native access-group names this deployment's operator has
  # configured (Decision 3 of
  # docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md):
  accessGroups:
    mcpUsersGroup: mcp-users
    mcpLiveBoardsGroup: mcp-live-boards
  # -- Shared secret between this service's /token orchestrator-session
  # grant (embedded-chatbot-option-b plan, Task 1) and orchestrator/'s own
  # token-acquisition step (Task 3) -- a SEPARATE credential from
  # internalCompleteToken above, same auto-generate-if-empty pattern (see
  # secret.yaml). REMOVED by Task 5 of the client-side-tokens plan, kept
  # here only because this task is a pure reconciliation, not yet the
  # redesign.
  orchestratorInternalToken: ""
  existingSecretOrchestratorInternalTokenKey: orchestrator-internal-token
```

```bash
git add charts/distant-signal/values.yaml
git rebase --continue
```

Every remaining commit (`4cae0b8`, `940cc8e`, `c8e0178`) rebases clean —
`git rebase` will not stop again.

- [ ] **Step 4: Verify the reconciled branch builds and its own pre-existing tests still pass**

```bash
cargo check --workspace
cargo test -p api chatbot -- --ignored --test-threads=1   # requires DATABASE_URL; skip if none available, note in the PR description instead
helm template charts/distant-signal --set orchestrator.enabled=true --set railMcp.enabled=true --set orchestrator.anthropicApiKey=test --set railMcp.publicUrl=https://example.com --set railMcp.frontendOrigin=https://example.com > /dev/null
cd frontend && npm test && npx tsc --noEmit && cd ..
```

- [ ] **Step 5: Confirm no other conflicts were silently mis-resolved**

```bash
git diff main...embedded-chatbot-option-b-client-side-tokens --stat
```

Compare the file list against the original branch's own 5-commit stat
(`orchestrator/`'s full tree, `crates/api/src/{auth,data/users}.rs`,
`crates/api/src/routes/{chatbot,mod}.rs`, the migration,
`frontend/app/{chat,api/chat}/`, `frontend/components/ChatPanel*`,
`frontend/lib/{api,types}.ts`, `charts/distant-signal/**`,
`.github/workflows/containers.yml`, `docker-compose.yml`,
`{dev,local}.env.example`) — nothing should be missing, and nothing from
`main`'s own unrelated 31 commits should appear as newly modified by this
diff (that would indicate an over-broad conflict resolution).

Nothing to commit here beyond what the rebase itself already recorded —
the rebase **is** the commit history for this task.

---

## Task 2: Reconcile `distant-signal-mcp`'s Option B branch onto current `master` (SEPARATE REPOSITORY)

**This task's files live in `distant-signal-mcp`, a completely separate git
repository, checked out at `/workspaces/distant-signal-mcp` in this
environment.** Run every step below from that repository's own working
copy, not this worktree.

**Files:**
- Modify (during rebase, conflict resolution only): `src/config.ts`, `test/config.test.ts`

**Interfaces:**
- Produces: a branch (`embedded-chatbot-option-b-client-side-tokens`,
  created from `feat/embedded-chatbot-option-b`, rebased onto current
  `master`) carrying both of that branch's original commits, conflict-free,
  with `master`'s 8 intervening commits (the `mcp-server-oauth-access-groups`
  work and the new GitHub Actions CI workflow) fully present and untouched.
- **Depends on:** nothing — independently reconcilable in parallel with
  Task 1.

The branch `feat/embedded-chatbot-option-b` carries 2 commits not on
`master`: `a42ea8c` (the orchestrator-session grant itself — this is where
the conflict surfaces) and `4ea9261` (a doc-comment-only update, rebases
clean).

- [ ] **Step 1: Start the rebase**

```bash
cd /workspaces/distant-signal-mcp
git checkout feat/embedded-chatbot-option-b
git checkout -b embedded-chatbot-option-b-client-side-tokens
git rebase master
```

- [ ] **Step 2: Resolve the conflict — `src/config.ts`, surfaces on commit `a42ea8c`**

The conflict is inside `loadConfig()`'s returned object literal, in the
`oauth: { ... }` block. `master`'s own `accessGroups` sibling field and
this branch's own `oauth.orchestratorInternalToken` field both touch the
same region. Resolve to:

```typescript
        oauth: {
            redisUrl: required(env, 'OAUTH_REDIS_URL'),
            issuer: publicUrl,
            internalCompleteToken: required(env, 'OAUTH_INTERNAL_COMPLETE_TOKEN'),
            orchestratorInternalToken: required(env, 'OAUTH_ORCHESTRATOR_INTERNAL_TOKEN')
        },
        accessGroups: {
            mcpUsersGroup: optionalString(env, 'MCP_USERS_GROUP', 'mcp-users'),
            mcpLiveBoardsGroup: optionalString(env, 'MCP_LIVE_BOARDS_GROUP', 'mcp-live-boards')
        }
```

(The `Config` interface's own `oauth.orchestratorInternalToken: string`
type field and its doc comment merge automatically clean — no action
needed there, only inside `loadConfig()`'s function body above.)

```bash
git add src/config.ts
git rebase --continue
```

- [ ] **Step 3: Resolve the conflict — `test/config.test.ts`, same commit**

Same shape as Task 1 Step 2 — two independent `it()` blocks appended at
the same point. **Keep both.** Resolve to:

```typescript
    it('defaults accessGroups to mcp-users/mcp-live-boards when unset', () => {
        const config = loadConfig(valid);
        expect(config.accessGroups).toEqual({ mcpUsersGroup: 'mcp-users', mcpLiveBoardsGroup: 'mcp-live-boards' });
    });

    it('reads MCP_USERS_GROUP/MCP_LIVE_BOARDS_GROUP when set, matching charts/distant-signal/values.yaml\'s own env var names', () => {
        const config = loadConfig({ ...valid, MCP_USERS_GROUP: 'custom-users', MCP_LIVE_BOARDS_GROUP: 'custom-live-boards' });
        expect(config.accessGroups).toEqual({ mcpUsersGroup: 'custom-users', mcpLiveBoardsGroup: 'custom-live-boards' });
    });

    it('parses OAUTH_ORCHESTRATOR_INTERNAL_TOKEN, a separate secret from OAUTH_INTERNAL_COMPLETE_TOKEN', () => {
        expect(loadConfig(valid).oauth.orchestratorInternalToken).toBe('orchestrator-internal-token-for-tests');
    });

    it('names OAUTH_ORCHESTRATOR_INTERNAL_TOKEN when it is missing', () => {
        const { OAUTH_ORCHESTRATOR_INTERNAL_TOKEN, ...rest } = valid;
        expect(() => loadConfig(rest)).toThrow(/OAUTH_ORCHESTRATOR_INTERNAL_TOKEN/);
    });
```

(`valid`'s own fixture object, defined earlier in the file, already carries
`OAUTH_ORCHESTRATOR_INTERNAL_TOKEN: 'orchestrator-internal-token-for-tests'`
from this branch's own commit — that line merges automatically clean, no
action needed.)

```bash
git add test/config.test.ts
git rebase --continue
```

The second commit (`4ea9261`) rebases clean — `git rebase` will not stop
again.

- [ ] **Step 4: Verify**

```bash
npm run typecheck
npm test
```

All pre-existing tests (including the now-doubly-confirmed `accessGroups`
tests and `test/oauth-orchestrator-grant.test.ts`'s 12 tests) must pass.
`test/oauth-orchestrator-grant.test.ts` still exists and still passes at
the end of this task — it is only removed in Task 4, deliberately kept
here so this task stays a pure reconciliation.

Nothing to commit here beyond the rebase itself.

---

## Task 3: `distant-signal-mcp` — add CORS to `/mcp` (SEPARATE REPOSITORY)

**Run from `/workspaces/distant-signal-mcp`, on top of Task 2's branch.**

**Files:**
- Modify: `src/app.ts`, `package.json`
- Test: `test/app.test.ts` (add cases; do not touch its existing ones)

**Interfaces:**
- Produces: `/mcp` now sends `Access-Control-Allow-Origin:
  <config.frontendOrigin.origin>` for a request whose `Origin` header
  matches, and omits that header entirely for any other origin — a real,
  additive behavior change confirmed as a genuine gap by this session's own
  re-inspection of `src/app.ts` (no `cors` import anywhere in the file
  today) and the vendored SDK's `server/streamableHttp.js` (no `cors`
  import there either).
- **Depends on:** Task 2 (this repo's own reconciled branch).

`config.frontendOrigin` is already a `URL` instance (`src/config.ts:12`,
`requiredUrl(env, 'DS_FRONTEND_ORIGIN')`) — `.origin` is a standard WHATWG
`URL` getter, already confirmed present, no new config plumbing needed.

- [ ] **Step 1: Add the `cors` dependency**

`cors@2.8.6` is already present in `node_modules` (hoisted transitively
via the MCP SDK's own vendored handlers), but is not a direct dependency
of this package — add it explicitly since `src/app.ts` will now import it
directly:

```bash
npm install cors@^2.8.6
npm install --save-dev @types/cors@^2.8.17
```

- [ ] **Step 2: Mount `cors()` on `/mcp`, scoped to `config.frontendOrigin`**

In `src/app.ts`, add the import alongside the existing ones:

```typescript
import cors from 'cors';
```

Then change the `/mcp` mount (currently `app.all('/mcp', addressLimiter,
express.json(), auth, userLimiter, async (req, res) => { ... })`) to put
CORS first, before the address rate limiter — a CORS preflight (`OPTIONS`)
should never count against that limiter, and `cors()` itself will
short-circuit and answer the preflight directly without calling `next()`,
so `addressLimiter` never runs for an `OPTIONS` request either way once
this is in place:

```typescript
    app.all(
        '/mcp',
        cors({ origin: config.frontendOrigin.origin }),
        addressLimiter,
        express.json(),
        auth,
        userLimiter,
        async (req, res) => {
            // ... unchanged
        }
    );
```

No other file changes — `/register`, `/token`, `/revoke`, and discovery
already get the SDK's own permissive `cors()` default (unmodified by this
task, per the design doc's Decision 2: those need to stay open to
arbitrary external MCP clients). `/authorize` needs no CORS at all (reached
by top-level navigation, not `fetch`).

- [ ] **Step 3: Tests — `test/app.test.ts`**

Add two new cases inside the existing `/mcp` describe block (do not modify
any existing case):

```typescript
    it('CORS-allows the configured frontend origin on /mcp', async () => {
        const { app: application } = fullApp();
        const res = await request(application)
            .options('/mcp')
            .set('Origin', FRONTEND_ORIGIN.href.replace(/\/$/, ''))
            .set('Access-Control-Request-Method', 'POST');
        expect(res.headers['access-control-allow-origin']).toBe(FRONTEND_ORIGIN.href.replace(/\/$/, ''));
    });

    it('does not reflect a different origin on /mcp', async () => {
        const { app: application } = fullApp();
        const res = await request(application)
            .options('/mcp')
            .set('Origin', 'https://evil.example.com')
            .set('Access-Control-Request-Method', 'POST');
        expect(res.headers['access-control-allow-origin']).toBeUndefined();
    });
```

(`FRONTEND_ORIGIN` and `fullApp()` are this file's own existing fixtures —
confirm their exact names against the file at the time this task runs;
`FRONTEND_ORIGIN` was `new URL('https://status.example.com')` as of Task
2's reconciliation.)

- [ ] **Step 4: Run, commit**

```bash
npm run typecheck
npm test
git add package.json package-lock.json src/app.ts test/app.test.ts
git commit -m "Add CORS to /mcp, scoped to config.frontendOrigin (a real gap, not a wildcard)"
```

---

## Task 4: `distant-signal-mcp` — remove the `urn:distant-signal:orchestrator-session` grant (SEPARATE REPOSITORY)

**Run from `/workspaces/distant-signal-mcp`, on top of Task 3's commit.**

**Files:**
- Delete: `src/oauth/orchestratorGrant.ts`, `test/oauth-orchestrator-grant.test.ts`
- Modify: `src/app.ts`, `src/config.ts`, `src/oauth/internal.ts`, `test/app.test.ts`, `test/config.test.ts`

**Interfaces:**
- Produces: `/token` accepts only `grant_type=authorization_code` (and
  `refresh_token`, unchanged, both handled entirely by the MCP SDK's own
  vendored `tokenHandler`) — the private-use
  `urn:distant-signal:orchestrator-session` grant type and its
  `X-Orchestrator-Internal-Token` gate no longer exist anywhere in this
  adapter.
- **Depends on:** Task 3 (build on the CORS commit, not a fresh branch off
  Task 2 — both tasks touch `src/app.ts`, sequence them to avoid a
  redundant conflict).

- [ ] **Step 1: Delete the grant module and its tests**

```bash
rm src/oauth/orchestratorGrant.ts test/oauth-orchestrator-grant.test.ts
```

- [ ] **Step 2: Remove the mount point in `src/app.ts`**

Remove the import:

```typescript
import { registerOrchestratorGrant } from './oauth/orchestratorGrant.js';
```

Remove the mount block (immediately before `app.use(registerOauthRouter(oauthProvider));`):

```typescript
    app.use('/token', registerOrchestratorGrant({
        store: oauthStore,
        publicUrl: config.publicUrl,
        orchestratorInternalToken: config.oauth.orchestratorInternalToken,
        dsBaseUrl: config.ds.baseUrl,
        fetchImpl
    }));
```

`registerOauthRouter(oauthProvider)` (the SDK's own `/token` handler) is
now the only thing mounted at `/token`.

- [ ] **Step 3: Remove `orchestratorInternalToken` from `src/config.ts`**

Remove the `oauth.orchestratorInternalToken: string` field (and its doc
comment) from the `Config` interface, and remove it from `loadConfig()`'s
returned object:

```typescript
        oauth: {
            redisUrl: required(env, 'OAUTH_REDIS_URL'),
            issuer: publicUrl,
            internalCompleteToken: required(env, 'OAUTH_INTERNAL_COMPLETE_TOKEN')
        },
        accessGroups: {
            mcpUsersGroup: optionalString(env, 'MCP_USERS_GROUP', 'mcp-users'),
            mcpLiveBoardsGroup: optionalString(env, 'MCP_LIVE_BOARDS_GROUP', 'mcp-live-boards')
        }
```

- [ ] **Step 4: Revert `src/oauth/internal.ts`'s exports back to unexported**

These were exported specifically for `orchestratorGrant.ts`'s reuse (each
carries a doc comment saying so) — once that file is gone, keeping them
exported is stale, misleading surface area. Revert:

```typescript
function timingSafeTokenEqual(provided: string, expected: string): boolean {
```

(drop the `export` keyword and the "Exported for reuse by
src/oauth/orchestratorGrant.ts..." doc comment above it, restoring the
plain original doc comment about the timing-safe-compare rationale), and:

```typescript
async function lookupDsUserId(dsBaseUrl: string, sessionCookieValue: string, fetchImpl: typeof fetch): Promise<string | undefined> {
```

(drop `export` and the "Exported for reuse..." doc comment the same way).

- [ ] **Step 5: Remove `OAUTH_ORCHESTRATOR_INTERNAL_TOKEN` from test fixtures**

`test/app.test.ts`: remove `OAUTH_ORCHESTRATOR_INTERNAL_TOKEN:
'orchestrator-internal-token-for-tests'` from its config fixture object.

`test/config.test.ts`: remove the same line from its `valid` fixture, and
delete the two tests Task 2 Step 3 kept (`'parses
OAUTH_ORCHESTRATOR_INTERNAL_TOKEN...'` and `'names
OAUTH_ORCHESTRATOR_INTERNAL_TOKEN when it is missing'`) — the field they
tested no longer exists. Leave the two `accessGroups` tests untouched.

- [ ] **Step 6: Run, commit**

```bash
npm run typecheck
npm test
git add -A
git commit -m "Remove the orchestrator-session grant -- no caller left once the browser does its own interactive OAuth"
```

Real, acknowledged loss: 12 tests deleted (`test/oauth-orchestrator-grant.test.ts`),
per the design doc's own Decision 5 and this plan's Global Constraints.

- [ ] **Step 7: Open a PR in `distant-signal-mcp`'s own repository** (or push
  to whatever remote the real deployment target uses — this environment's
  checkout has no `origin` configured, adjust accordingly):

```bash
git push -u origin embedded-chatbot-option-b-client-side-tokens
```

---

## Task 5: Remove `orchestrator/` entirely (this repo)

**Files:**
- Delete (whole directory): `orchestrator/` (`package.json`, `tsconfig*.json`,
  `Dockerfile`, `src/{app,chat,config,dsClient,index,mcpToken}.ts`,
  `test/{chat,dsClient,mcpToken}.test.ts`, `vitest.config.ts`, `.gitignore`,
  `package-lock.json`)
- Delete: `frontend/app/api/chat/route.ts`, `frontend/app/api/chat/route.test.ts`
  (the same-origin SSE proxy — obsolete once the browser talks to
  `distant-signal-mcp`/Anthropic directly, no server-side relay left for it
  to proxy to)
- Delete: `charts/distant-signal/templates/orchestrator-deployment.yaml`,
  `charts/distant-signal/templates/orchestrator-service.yaml`
- Modify: `charts/distant-signal/values.yaml` (remove the `orchestrator:`
  block and `railMcp.orchestratorInternalToken`/
  `railMcp.existingSecretOrchestratorInternalTokenKey`),
  `charts/distant-signal/templates/secret.yaml` (remove the
  `orchestrator-internal-token`/`anthropic-api-key` generation blocks),
  `charts/distant-signal/templates/_helpers.tpl` (remove
  `orchestratorFullname`/secret-key-resolver helpers this branch added),
  `charts/distant-signal/templates/frontend-deployment.yaml` (remove the
  `ORCHESTRATOR_BASE_URL` env var), `.github/workflows/containers.yml`
  (remove the `chat-orchestrator` matrix entry), `docker-compose.yml` /
  `docker-compose.dev.yml` (remove the `orchestrator` service block),
  `dev.env.example` / `local.env.example` (remove `ORCHESTRATOR_*`/
  `OAUTH_ORCHESTRATOR_INTERNAL_TOKEN` entries)

**Interfaces:**
- Produces: no `orchestrator/` directory, no `orchestrator` Helm values,
  no `chat-orchestrator` CI matrix entry, no same-origin SSE proxy route.
  `frontend/app/chat/page.tsx` and `getChatbotAccess()` are untouched
  (Global Constraints) — only their downstream (`ChatPanel.tsx`, Task 10;
  the API proxy, deleted here) changes.
- **Depends on:** Task 1 (this repo's own reconciled branch). Independent
  of Tasks 3-4 (separate repository, separate deploy unit) — can run in
  parallel with them.

- [ ] **Step 1: Delete the service directory and the SSE proxy**

```bash
rm -rf orchestrator/
rm frontend/app/api/chat/route.ts frontend/app/api/chat/route.test.ts
rmdir frontend/app/api/chat 2>/dev/null || true
```

Real, acknowledged loss: 18 `orchestrator/` tests (6+7+5 across
`chat`/`dsClient`/`mcpToken`) plus 6 `route.test.ts` tests — 24 tests
total, deleted, not ported (Global Constraints).

- [ ] **Step 2: `values.yaml` — remove the `orchestrator:` block and `railMcp.orchestratorInternalToken`**

Remove the entire `orchestrator:` top-level block (added by `92c7576`,
reconciled untouched by Task 1). Remove, from `railMcp:`, the two lines
Task 1 Step 3 deliberately kept for reconciliation purposes only:

```yaml
  orchestratorInternalToken: ""
  existingSecretOrchestratorInternalTokenKey: orchestrator-internal-token
```

`railMcp.accessGroups` (the block that conflicted with it) stays exactly
as `main` already has it.

- [ ] **Step 3: `secret.yaml` — remove the two generation blocks Task 1's reconciled branch carries**

Remove:

```yaml
{{- if .Values.railMcp.enabled }}
{{- $orchestratorToken := .Values.railMcp.orchestratorInternalToken | default (get $existingData "orchestrator-internal-token" | b64dec) | default (randAlphaNum 32) -}}
{{- $_ := set $data "orchestrator-internal-token" ($orchestratorToken | b64enc) -}}
{{- end }}
{{- if .Values.orchestrator.enabled }}
{{- $apiKey := .Values.orchestrator.anthropicApiKey | default (get $existingData "anthropic-api-key" | b64dec) -}}
{{- if not $apiKey }}{{ fail "orchestrator.anthropicApiKey (or an existingSecret providing it) is required when orchestrator.enabled is true" }}{{ end }}
{{- $_ := set $data "anthropic-api-key" ($apiKey | b64enc) -}}
{{- end }}
```

- [ ] **Step 4: `_helpers.tpl` — remove the orchestrator-specific helpers**

Remove `orchestratorFullname` and any `orchestrator*SecretKey` helpers
added by `92c7576`.

- [ ] **Step 5: `frontend-deployment.yaml` — remove `ORCHESTRATOR_BASE_URL`**

Remove the conditional env var block this branch added
(`{{- if .Values.orchestrator.enabled }}` guarding `ORCHESTRATOR_BASE_URL`).
`NEXT_PUBLIC_RAILMCP_PUBLIC_URL` (pre-existing, unrelated to
`orchestrator/`) is untouched — Task 8/10 reuse it directly.

- [ ] **Step 6: `containers.yml` — remove the `chat-orchestrator` matrix entry**

- [ ] **Step 7: `docker-compose.yml`/`docker-compose.dev.yml` — remove the `orchestrator` service block**

- [ ] **Step 8: `dev.env.example`/`local.env.example` — remove the orchestrator-related entries**

`ORCHESTRATOR_BASE_URL`, `OAUTH_ORCHESTRATOR_INTERNAL_TOKEN`,
`ANTHROPIC_API_KEY` (the DS-owned one — a per-user key entered in the
browser, Task 9, needs no server-side env var).

- [ ] **Step 9: Verify, commit**

```bash
helm template charts/distant-signal --set railMcp.enabled=true --set railMcp.publicUrl=https://example.com --set railMcp.frontendOrigin=https://example.com > /dev/null
grep -rn "orchestrator" charts/ .github/workflows/containers.yml docker-compose.yml dev.env.example local.env.example || echo "clean"
cd frontend && npm test && npx tsc --noEmit && cd ..
git add -A
git commit -m "Remove orchestrator/ entirely -- the browser is its own MCP client and Anthropic caller now (Decision 3)"
```

The `grep` above should print only `echo "clean"`'s own output — any
remaining hit is a leftover reference this step missed.

---

## Task 6: Repurpose `chatbot_allowed_users`' doc comments (this repo)

**Files:**
- Modify: `crates/api/migrations/20260902110000_chatbot_allowed_users.sql`,
  `crates/api/src/routes/chatbot.rs`

**Interfaces:**
- Produces: no schema or behavior change — doc-comment-only. The table,
  extractor, route, and its three-state response shape (`200`/`401`/`403`)
  are all unchanged.
- **Depends on:** Task 1 (reconciled branch); logically follows Task 5
  (once `orchestrator/`'s own consumer of this gate is gone,
  `frontend/app/chat/page.tsx` is the only caller left, which is what the
  new framing below states as fact, not aspiration) but has no file
  overlap with it — can run in parallel.

- [ ] **Step 1: Migration header comment**

In `crates/api/migrations/20260902110000_chatbot_allowed_users.sql`,
replace:

```sql
-- -------------------------------------------------------------------------
-- chatbot_allowed_users: the DS-hosted chat orchestrator (Option B)'s cost/
-- access gate. See
-- docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b.md's Task 2
-- and docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md's
-- Decision 5.
```

with:

```sql
-- -------------------------------------------------------------------------
-- chatbot_allowed_users: a beta/feature-flag gate for the /chat page's own
-- visibility -- NOT a spend-protection mechanism (it was originally built
-- as one, back when Option B held a DS-funded Anthropic key server-side;
-- see docs/superpowers/specs/2026-09-02-embedded-chatbot-option-b-client-side-tokens-design.md's
-- Decision 4 for why that framing stopped being accurate once each user
-- pays for their own Anthropic usage directly). See
-- docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b.md's Task 2
-- for this table's original shape (unchanged) and
-- docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b-client-side-tokens.md's
-- Task 6 for this re-framing.
```

(Leave the rest of the file's comment — the `ON DELETE CASCADE` rationale
— unchanged; it was never spend-protection framing to begin with.)

- [ ] **Step 2: `crates/api/src/routes/chatbot.rs` module doc comment**

Replace:

```rust
//! `/public/chatbot/access` -- the DS-hosted chat orchestrator (Option B)'s
//! own allowlist check. See
//! docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b.md's Task 2
//! and docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md's
//! Decision 5.
//!
//! Two callers: `frontend/app/chat/page.tsx` (Task 5, a page-load gate) and
//! `orchestrator/` (Task 3, the actual cost-protecting check, since a
//! request can reach the orchestrator without ever rendering the page).
```

with:

```rust
//! `/public/chatbot/access` -- a beta/feature-flag gate for the `/chat`
//! page's own visibility. NOT spend-protection: that was this table's
//! original purpose (dual-mode design's Decision 5), back when Option B
//! held a DS-funded Anthropic key server-side. Once each user supplies
//! their own Anthropic key directly to their own browser (see
//! docs/superpowers/specs/2026-09-02-embedded-chatbot-option-b-client-side-tokens-design.md's
//! Decision 4), there is no DS spend left to protect -- this is now purely
//! a soft-launch/access-control gate, independent from and not a proxy for
//! `distant-signal-mcp`'s own `mcp-users`/`mcp-live-boards` access groups
//! (which gate the tools themselves, for a materially different
//! population -- Option C's arbitrary Claude.ai users included).
//!
//! One caller: `frontend/app/chat/page.tsx`'s own page-load gate. (The
//! former second caller, `orchestrator/`'s `checkChatbotAccess` -- "the
//! actual cost-protecting check, since a request can reach the
//! orchestrator without ever rendering the page" -- no longer exists;
//! `orchestrator/` was removed entirely, see the client-side-tokens plan's
//! Task 5.)
```

- [ ] **Step 3: Verify, commit**

```bash
cargo check -p api
git add crates/api/migrations/20260902110000_chatbot_allowed_users.sql crates/api/src/routes/chatbot.rs
git commit -m "Repurpose chatbot_allowed_users' doc comments: beta gate, not spend protection (Decision 4)"
```

No test changes — the three-state behavior this doc comment describes is
unchanged; only its stated *reason for existing* changed.

---

## Task 7: `frontend` — dependencies + `BrowserMcpOAuthProvider` (localStorage)

**Files:**
- Modify: `frontend/package.json`
- Create: `frontend/lib/mcpOAuthProvider.ts`
- Test: `frontend/lib/mcpOAuthProvider.test.ts`

**Interfaces:**
- Produces: `class BrowserMcpOAuthProvider implements OAuthClientProvider`
  (from `@modelcontextprotocol/sdk/client/auth.js`) — `redirectUrl`,
  `clientMetadata`, `clientInformation()`/`saveClientInformation()`,
  `tokens()`/`saveTokens()`, `redirectToAuthorization()`,
  `saveCodeVerifier()`/`codeVerifier()`, all backed by `localStorage`
  (matching `ThemeToggle.tsx`/`PrideToggle.tsx`'s own precedent, per the
  design doc's Decision 6).
- Consumed by: Task 8 (the callback route, via the SDK's own `auth()`
  helper) and Task 10 (`ChatPanel.tsx`, via
  `StreamableHTTPClientTransport`'s `authProvider` option).
- **Depends on:** Task 1 (reconciled branch); no dependency on Tasks 3-6.

`@modelcontextprotocol/sdk`'s `client/auth.d.ts` (installed in
`distant-signal-mcp`, confirmed in this session, `v1.29.0`) declares
`OAuthClientProvider` with these required methods (the rest are optional,
not implemented here): `get redirectUrl()`, `get clientMetadata()`,
`clientInformation()`, `saveClientInformation()`, `tokens()`,
`saveTokens()`, `redirectToAuthorization(authorizationUrl: URL)`,
`saveCodeVerifier(codeVerifier: string)`, `codeVerifier()`.

- [ ] **Step 1: Add dependencies**

```bash
cd frontend
npm install @modelcontextprotocol/sdk@^1.29.0 @anthropic-ai/sdk@^0.123.0
cd ..
```

Import only the browser-bundlable subpaths throughout this plan —
`@modelcontextprotocol/sdk/client/index.js`,
`@modelcontextprotocol/sdk/client/streamableHttp.js`,
`@modelcontextprotocol/sdk/client/auth.js` — never the bare package root
or `@modelcontextprotocol/sdk/client/stdio.js` (the Node-only
child-process transport this design doc's own Current relevant state
confirmed `client/index.js` does not transitively import).

- [ ] **Step 2: Write the failing test**

```typescript
// frontend/lib/mcpOAuthProvider.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { BrowserMcpOAuthProvider } from './mcpOAuthProvider';

describe('BrowserMcpOAuthProvider', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('returns undefined tokens/clientInformation before anything is saved', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    expect(provider.tokens()).toBeUndefined();
    expect(provider.clientInformation()).toBeUndefined();
  });

  it('round-trips tokens through localStorage', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    const tokens = { access_token: 'abc123', token_type: 'Bearer' as const };
    provider.saveTokens(tokens);
    expect(provider.tokens()).toEqual(tokens);

    // A second provider instance reads the same persisted value -- proof
    // this isn't in-memory state, it's actually localStorage-backed.
    const reloaded = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    expect(reloaded.tokens()).toEqual(tokens);
  });

  it('round-trips client information through localStorage', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    const info = { client_id: 'c1', redirect_uris: ['https://status.example.com/chat/callback'] };
    provider.saveClientInformation(info as never);
    expect(provider.clientInformation()).toEqual(info);
  });

  it('round-trips the PKCE code verifier through localStorage', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    provider.saveCodeVerifier('a-verifier-value');
    expect(provider.codeVerifier()).toBe('a-verifier-value');
  });

  it('throws a clear error reading codeVerifier() before one was saved', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    expect(() => provider.codeVerifier()).toThrow(/no pkce code verifier/i);
  });

  it('exposes the redirect URL and clientMetadata this app registers with', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    expect(provider.redirectUrl).toBe('https://status.example.com/chat/callback');
    expect(provider.clientMetadata.redirect_uris).toEqual(['https://status.example.com/chat/callback']);
    expect(provider.clientMetadata.token_endpoint_auth_method).toBe('none');
  });
});
```

- [ ] **Step 3: Run to verify it fails**

```bash
cd frontend && npx vitest run lib/mcpOAuthProvider.test.ts
```

Expected: FAIL — `./mcpOAuthProvider` does not exist yet.

- [ ] **Step 4: Implement**

```typescript
// frontend/lib/mcpOAuthProvider.ts
import type { OAuthClientProvider } from '@modelcontextprotocol/sdk/client/auth.js';
import type {
  OAuthClientInformationFull,
  OAuthClientMetadata,
  OAuthTokens,
} from '@modelcontextprotocol/sdk/shared/auth.js';

const STORAGE_PREFIX = 'ds-mcp-oauth:';
const CLIENT_INFO_KEY = `${STORAGE_PREFIX}client-information`;
const TOKENS_KEY = `${STORAGE_PREFIX}tokens`;
const CODE_VERIFIER_KEY = `${STORAGE_PREFIX}code-verifier`;

/** A per-viewer, browser-local OAuth client against `distant-signal-mcp`'s
 * own OAuth 2.1 authorization server -- the same DCR/PKCE-only public-
 * client shape Claude Desktop already gets from `RailMcpOAuthProvider`,
 * just run inside the browser instead of a native app. Backed by
 * `localStorage`, matching this app's existing precedent
 * (`ThemeToggle.tsx`/`PrideToggle.tsx`) -- see the client-side-tokens
 * design doc's Decision 6 for why `localStorage` over `sessionStorage`/
 * IndexedDB.
 *
 * Implements the MCP SDK's own `OAuthClientProvider` interface
 * (`@modelcontextprotocol/sdk/client/auth.js`) so both the SDK's exported
 * `auth()` orchestrator (Task 8's callback route) and
 * `StreamableHTTPClientTransport`'s own `authProvider` option (Task 10's
 * `ChatPanel.tsx`) can drive it directly -- no hand-rolled redirect/
 * exchange/store sequence needed. */
export class BrowserMcpOAuthProvider implements OAuthClientProvider {
  constructor(private readonly callbackUrl: string) {}

  get redirectUrl(): string {
    return this.callbackUrl;
  }

  get clientMetadata(): OAuthClientMetadata {
    return {
      client_name: 'Distant Signal chat',
      redirect_uris: [this.callbackUrl],
      grant_types: ['authorization_code'],
      response_types: ['code'],
      // PKCE-only public client -- no secret, matching every other MCP
      // client this adapter's DCR (`RailMcpOAuthProvider.registerClient`)
      // ever issues.
      token_endpoint_auth_method: 'none',
    };
  }

  clientInformation(): OAuthClientInformationFull | undefined {
    return readJson<OAuthClientInformationFull>(CLIENT_INFO_KEY);
  }

  saveClientInformation(clientInformation: OAuthClientInformationFull): void {
    localStorage.setItem(CLIENT_INFO_KEY, JSON.stringify(clientInformation));
  }

  tokens(): OAuthTokens | undefined {
    return readJson<OAuthTokens>(TOKENS_KEY);
  }

  saveTokens(tokens: OAuthTokens): void {
    localStorage.setItem(TOKENS_KEY, JSON.stringify(tokens));
  }

  redirectToAuthorization(authorizationUrl: URL): void {
    window.location.href = authorizationUrl.toString();
  }

  saveCodeVerifier(codeVerifier: string): void {
    localStorage.setItem(CODE_VERIFIER_KEY, codeVerifier);
  }

  codeVerifier(): string {
    const verifier = localStorage.getItem(CODE_VERIFIER_KEY);
    if (!verifier) {
      throw new Error('No PKCE code verifier found in localStorage -- the authorization flow was not started from this browser');
    }
    return verifier;
  }
}

function readJson<T>(key: string): T | undefined {
  const raw = localStorage.getItem(key);
  if (!raw) return undefined;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return undefined;
  }
}
```

- [ ] **Step 5: Run to verify it passes**

```bash
cd frontend && npx vitest run lib/mcpOAuthProvider.test.ts
```

Expected: PASS, all 6 cases.

- [ ] **Step 6: Commit**

```bash
git add frontend/package.json frontend/package-lock.json frontend/lib/mcpOAuthProvider.ts frontend/lib/mcpOAuthProvider.test.ts
git commit -m "Add BrowserMcpOAuthProvider: localStorage-backed OAuthClientProvider for the SDK's browser client"
```

---

## Task 8: `frontend/app/chat/callback` — code exchange (NEW route)

**Files:**
- Create: `frontend/app/chat/callback/page.tsx`
- Test: `frontend/app/chat/callback/page.test.tsx`

**Interfaces:**
- Produces: `/chat/callback?code=...` — a client-rendered page that
  exchanges the authorization code via the SDK's own exported `auth()`
  function, then redirects to `/chat`.
- Consumed by: nothing in this repo (it's a terminal redirect target
  reached only via `distant-signal-mcp`'s own `/authorize` →
  `/connect-claude/authorize` consent bridge → redirect chain, Task 10's
  own trigger).
- **Depends on:** Task 7 (`BrowserMcpOAuthProvider`).

`NEXT_PUBLIC_RAILMCP_PUBLIC_URL` already exists (baked in at
container-start, `charts/distant-signal/templates/frontend-deployment.yaml`,
already consumed client-side-safely by `frontend/app/connect-claude/page.tsx`'s
own `railMcpPublicUrl()` helper) — no new env var or chart change needed
for this task or Task 10.

- [ ] **Step 1: Write the failing test**

```typescript
// frontend/app/chat/callback/page.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import ChatCallbackPage from './page';

const mockAuth = vi.fn();
const mockReplace = vi.fn();

vi.mock('@modelcontextprotocol/sdk/client/auth.js', () => ({
  auth: (...args: unknown[]) => mockAuth(...args),
}));
vi.mock('next/navigation', () => ({
  useRouter: () => ({ replace: mockReplace }),
}));

function renderAt(search: string) {
  window.history.pushState({}, '', `/chat/callback${search}`);
  return renderWithMantine(<ChatCallbackPage />);
}

describe('ChatCallbackPage', () => {
  beforeEach(() => {
    localStorage.clear();
    mockAuth.mockReset();
    mockReplace.mockReset();
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
  });

  it('exchanges the code and redirects to /chat on success', async () => {
    mockAuth.mockResolvedValue('AUTHORIZED');
    renderAt('?code=abc123');
    await waitFor(() => expect(mockReplace).toHaveBeenCalledWith('/chat'));
    expect(mockAuth).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ serverUrl: 'https://mcp.example.com', authorizationCode: 'abc123' }),
    );
  });

  it('shows an error and does not redirect when no code is present', async () => {
    renderAt('');
    expect(await screen.findByText(/no authorization code/i)).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it('shows an error when the exchange itself fails', async () => {
    mockAuth.mockRejectedValue(new Error('token exchange failed'));
    renderAt('?code=abc123');
    expect(await screen.findByText(/token exchange failed/i)).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it('shows an error when auth() returns REDIRECT instead of AUTHORIZED', async () => {
    mockAuth.mockResolvedValue('REDIRECT');
    renderAt('?code=abc123');
    expect(await screen.findByText(/did not complete/i)).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

```bash
cd frontend && npx vitest run app/chat/callback/page.test.tsx
```

Expected: FAIL — `./page` does not exist yet.

- [ ] **Step 3: Implement**

```tsx
// frontend/app/chat/callback/page.tsx
'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Stack, Text, Title } from '@mantine/core';
import { auth } from '@modelcontextprotocol/sdk/client/auth.js';
import { BrowserMcpOAuthProvider } from '@/lib/mcpOAuthProvider';

/** `railMcp`'s own public URL, baked in at container-start -- the same
 * env var `frontend/app/connect-claude/page.tsx`'s own `railMcpPublicUrl()`
 * already reads for exactly this purpose. Read fresh (not module-level)
 * for the same "picked up per render, not baked at module-load" reasoning
 * that page's own doc comment gives. */
function railMcpPublicUrl(): string {
  const url = process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL;
  if (!url) throw new Error('NEXT_PUBLIC_RAILMCP_PUBLIC_URL is not configured on this deployment');
  return url;
}

/** `/chat/callback` -- the redirect target `distant-signal-mcp`'s own
 * `/authorize` -> `/connect-claude/authorize` consent bridge sends the
 * browser back to once the user approves (client-side-tokens design doc,
 * Decisions 1/3, Architecture step 3). Exchanges the `code` query param
 * for a bearer token via the MCP SDK's own `auth()` orchestrator
 * (`@modelcontextprotocol/sdk/client/auth.js`) -- the SAME function
 * `StreamableHTTPClientTransport` calls internally on a 401, reused here
 * directly for the one-time authorization-code exchange, driven by
 * `BrowserMcpOAuthProvider` (Task 7) so the resulting tokens land in
 * `localStorage` the same way either caller would leave them. */
export default function ChatCallbackPage() {
  const router = useRouter();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const code = new URLSearchParams(window.location.search).get('code');
    if (!code) {
      setError('No authorization code was present in the callback URL.');
      return;
    }

    const provider = new BrowserMcpOAuthProvider(`${window.location.origin}/chat/callback`);
    auth(provider, { serverUrl: railMcpPublicUrl(), authorizationCode: code })
      .then((result) => {
        if (result === 'AUTHORIZED') {
          router.replace('/chat');
        } else {
          setError('Authorization did not complete. Please try connecting again from the Chat page.');
        }
      })
      .catch((err: unknown) => {
        setError(err instanceof Error ? err.message : 'Connecting to the rail data service failed.');
      });
  }, [router]);

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Connecting…</Title>
      {error ? <Text c="red">{error}</Text> : <Text c="dimmed">Finishing sign-in to the rail data service.</Text>}
    </Stack>
  );
}
```

- [ ] **Step 4: Run to verify it passes**

```bash
cd frontend && npx vitest run app/chat/callback/page.test.tsx
```

Expected: PASS, all 4 cases.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/chat/callback/
git commit -m "Add /chat/callback: exchanges the OAuth code, reuses the SDK's own auth() orchestrator"
```

---

## Task 9: Anthropic API key entry/storage UI + one-time disclosure

**Files:**
- Create: `frontend/lib/anthropicKey.ts`, `frontend/components/AnthropicKeySettings.tsx`
- Test: `frontend/lib/anthropicKey.test.ts`, `frontend/components/AnthropicKeySettings.test.tsx`

**Interfaces:**
- Produces: `getAnthropicApiKey(): string | null`,
  `setAnthropicApiKey(key: string): void`, `clearAnthropicApiKey(): void`
  (pure `localStorage` read/write); `<AnthropicKeySettings />` — a
  Mantine-based settings affordance with a masked input, save/clear
  actions, and the design doc's required one-time disclosure text.
- Consumed by: Task 10 (`ChatPanel.tsx` renders `AnthropicKeySettings` and
  reads `getAnthropicApiKey()` before constructing its Anthropic client).
- **Depends on:** Task 1 (reconciled branch); independent of Tasks 7-8.

The design doc's Decision 6 requires, verbatim in substance: the key is
stored only in the browser, sent only to Anthropic directly, and never
seen by any Distant Signal server — stated plainly before or at first key
entry.

- [ ] **Step 1: Write the failing test — storage helpers**

```typescript
// frontend/lib/anthropicKey.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { getAnthropicApiKey, setAnthropicApiKey, clearAnthropicApiKey } from './anthropicKey';

describe('anthropicKey storage', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('returns null before any key is set', () => {
    expect(getAnthropicApiKey()).toBeNull();
  });

  it('round-trips a key through localStorage', () => {
    setAnthropicApiKey('sk-ant-test-key');
    expect(getAnthropicApiKey()).toBe('sk-ant-test-key');
  });

  it('clears a stored key', () => {
    setAnthropicApiKey('sk-ant-test-key');
    clearAnthropicApiKey();
    expect(getAnthropicApiKey()).toBeNull();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

```bash
cd frontend && npx vitest run lib/anthropicKey.test.ts
```

Expected: FAIL — module does not exist yet.

- [ ] **Step 3: Implement the storage helpers**

```typescript
// frontend/lib/anthropicKey.ts
// A per-viewer, browser-local credential -- localStorage, same precedent
// as ThemeToggle.tsx/PrideToggle.tsx and BrowserMcpOAuthProvider (Task 7).
// NEVER sent to any Distant Signal server -- this module's only consumers
// are Task 10's direct-to-Anthropic client construction and
// AnthropicKeySettings' own UI below. See the client-side-tokens design
// doc's Decision 6 for why localStorage over sessionStorage/IndexedDB.
const STORAGE_KEY = 'ds-anthropic-api-key';

export function getAnthropicApiKey(): string | null {
  return localStorage.getItem(STORAGE_KEY);
}

export function setAnthropicApiKey(key: string): void {
  localStorage.setItem(STORAGE_KEY, key);
}

export function clearAnthropicApiKey(): void {
  localStorage.removeItem(STORAGE_KEY);
}
```

- [ ] **Step 4: Run to verify it passes**

```bash
cd frontend && npx vitest run lib/anthropicKey.test.ts
```

Expected: PASS.

- [ ] **Step 5: Write the failing test — settings UI**

```typescript
// frontend/components/AnthropicKeySettings.test.tsx
import { describe, it, expect, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AnthropicKeySettings } from './AnthropicKeySettings';
import { getAnthropicApiKey } from '@/lib/anthropicKey';

describe('AnthropicKeySettings', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('shows the one-time disclosure text and no key set state', () => {
    renderWithMantine(<AnthropicKeySettings />);
    expect(screen.getByText(/stored only in your browser/i)).toBeInTheDocument();
    expect(screen.getByText(/never (sent to|seen by) (any )?distant signal/i)).toBeInTheDocument();
    expect(screen.getByText(/no key set/i)).toBeInTheDocument();
  });

  it('saves a key entered in the input', () => {
    renderWithMantine(<AnthropicKeySettings />);
    fireEvent.change(screen.getByLabelText(/anthropic api key/i), { target: { value: 'sk-ant-abc123' } });
    fireEvent.click(screen.getByRole('button', { name: /save/i }));
    expect(getAnthropicApiKey()).toBe('sk-ant-abc123');
    expect(screen.getByText(/key saved/i)).toBeInTheDocument();
  });

  it('masks a previously saved key rather than showing it in full', () => {
    setAnthropicApiKeyForTest('sk-ant-abcdefghijklmnop');
    renderWithMantine(<AnthropicKeySettings />);
    expect(screen.queryByDisplayValue('sk-ant-abcdefghijklmnop')).not.toBeInTheDocument();
    expect(screen.getByText(/key saved/i)).toBeInTheDocument();
  });

  it('clears a saved key', () => {
    setAnthropicApiKeyForTest('sk-ant-abc123');
    renderWithMantine(<AnthropicKeySettings />);
    fireEvent.click(screen.getByRole('button', { name: /clear/i }));
    expect(getAnthropicApiKey()).toBeNull();
    expect(screen.getByText(/no key set/i)).toBeInTheDocument();
  });
});

function setAnthropicApiKeyForTest(key: string) {
  localStorage.setItem('ds-anthropic-api-key', key);
}
```

- [ ] **Step 6: Run to verify it fails**

```bash
cd frontend && npx vitest run components/AnthropicKeySettings.test.tsx
```

Expected: FAIL — component does not exist yet.

- [ ] **Step 7: Implement**

```tsx
// frontend/components/AnthropicKeySettings.tsx
'use client';

import { useState } from 'react';
import { Alert, Button, Group, PasswordInput, Stack, Text } from '@mantine/core';
import { getAnthropicApiKey, setAnthropicApiKey, clearAnthropicApiKey } from '@/lib/anthropicKey';

/** The Chat page's own settings affordance for a user's Anthropic API key
 * (client-side-tokens design doc, Decision 6). Deliberately inline inside
 * /chat, not a new top-level route -- a single-field control, not a
 * page-sized concern.
 *
 * The disclosure text below is a real trust requirement, not polish: the
 * whole point of this redesign is that the key never reaches a Distant
 * Signal server, which only has value if the user is told it's true. */
export function AnthropicKeySettings() {
  const [hasKey, setHasKey] = useState(() => getAnthropicApiKey() !== null);
  const [input, setInput] = useState('');
  const [savedMessage, setSavedMessage] = useState(false);

  function handleSave() {
    if (!input.trim()) return;
    setAnthropicApiKey(input.trim());
    setInput('');
    setHasKey(true);
    setSavedMessage(true);
  }

  function handleClear() {
    clearAnthropicApiKey();
    setHasKey(false);
    setSavedMessage(false);
  }

  return (
    <Stack gap="xs">
      <Alert color="blue" variant="light">
        Your Anthropic API key is stored only in your browser (localStorage)
        and sent only to Anthropic directly when you chat -- it is never
        sent to or seen by any Distant Signal server.
      </Alert>
      <Text size="sm" fw={500}>
        {hasKey ? 'Key saved.' : 'No key set.'}
      </Text>
      <Group align="flex-end">
        <PasswordInput
          label="Anthropic API key"
          placeholder="sk-ant-..."
          value={input}
          onChange={(e) => setInput(e.currentTarget.value)}
          style={{ flex: 1 }}
        />
        <Button onClick={handleSave} disabled={!input.trim()}>
          Save
        </Button>
        {hasKey && (
          <Button variant="subtle" color="red" onClick={handleClear}>
            Clear
          </Button>
        )}
      </Group>
      {savedMessage && (
        <Text size="xs" c="dimmed">
          Key saved to this browser.
        </Text>
      )}
    </Stack>
  );
}
```

- [ ] **Step 8: Run to verify it passes**

```bash
cd frontend && npx vitest run components/AnthropicKeySettings.test.tsx lib/anthropicKey.test.ts
```

Expected: PASS, all 7 cases combined.

- [ ] **Step 9: Commit**

```bash
git add frontend/lib/anthropicKey.ts frontend/lib/anthropicKey.test.ts frontend/components/AnthropicKeySettings.tsx frontend/components/AnthropicKeySettings.test.tsx
git commit -m "Add the Anthropic API key settings affordance + one-time disclosure (Decision 6)"
```

---

## Task 10: Rewrite `ChatPanel.tsx` — the browser-side tool-calling loop

**Files:**
- Create: `frontend/lib/chatTurn.ts` (relocated from `orchestrator/src/chat.ts`, now deleted by Task 5)
- Modify: `frontend/components/ChatPanel.tsx`
- Test: `frontend/lib/chatTurn.test.ts`, `frontend/components/ChatPanel.test.tsx` (full rewrite)

**Interfaces:**
- Produces: `runChatTurn(opts: RunChatTurnOptions): AsyncGenerator<ChatEvent>`
  — same signature shape as `orchestrator/src/chat.ts`'s own
  `runChatTurn` (Decision 1: relocated "close to verbatim in shape", not
  redesigned), except `opts.anthropic` is now constructed with
  `dangerouslyAllowBrowser: true` using the user's own key (Task 9) and
  `opts.mcpBearerToken`/`opts.mcpUrl` feed a `StreamableHTTPClientTransport`
  configured with `BrowserMcpOAuthProvider` (Task 7) as its `authProvider`
  instead of a static bearer header, so an expired token triggers the
  transport's own automatic re-auth-on-401 rather than a hard failure.
- Consumed by: `ChatPanel.tsx`, which also renders `AnthropicKeySettings`
  (Task 9) and surfaces the three distinct error states Decision 6
  requires (Anthropic key rejected, MCP token expired/revoked, tool-level
  error).
- **Depends on:** Tasks 7, 9 (both providers this task wires together);
  logically depends on Task 5 (orchestrator/'s own `chat.ts` no longer
  exists to import from — this task's own `chatTurn.ts` is the sole
  surviving copy of that logic) but has no file conflict with it.

- [ ] **Step 1: `frontend/lib/chatTurn.ts` — relocate the loop**

This is `orchestrator/src/chat.ts`'s own `runChatTurn` (Task 5 deleted the
original with the rest of `orchestrator/`), copied close to verbatim per
Decision 1 — the only real changes are the Anthropic client construction
(now takes the user's own key, `dangerouslyAllowBrowser: true`) and the
MCP transport construction (now takes an `authProvider` instead of a
static `Authorization` header, so `StreamableHTTPClientTransport`'s own
built-in auto-reauth-on-401 applies):

```typescript
// frontend/lib/chatTurn.ts
/** The Anthropic Messages API tool-calling loop against distant-signal-mcp's
 * six tools -- relocated verbatim in shape from orchestrator/src/chat.ts
 * (now deleted, see this plan's Task 5) into the browser, per the
 * client-side-tokens design doc's Decision 1. The loop logic itself is
 * NOT redesigned here, only where it runs and how its two clients
 * authenticate. */
import Anthropic from '@anthropic-ai/sdk';
import type { BetaRunnableTool } from '@anthropic-ai/sdk/lib/tools/BetaRunnableTool';
import { Client as McpClient } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import type { OAuthClientProvider } from '@modelcontextprotocol/sdk/client/auth.js';

export type ChatEvent =
  | { type: 'text-delta'; text: string }
  | { type: 'tool-result'; toolName: string; structuredContent?: unknown }
  | { type: 'done' };

// Same unresearched-starting-figure posture orchestrator/src/chat.ts's own
// SYSTEM_PROMPT/MAX_ITERATIONS carried -- the design doc's own "Explicitly
// out of scope" list still leaves this un-designed.
const SYSTEM_PROMPT =
  'You are the Distant Signal assistant, helping a UK rail passenger check ' +
  'live departures, arrivals, service disruptions, and plan journeys. Use ' +
  'the available tools to answer with current, accurate information rather ' +
  'than guessing. Keep answers concise and focused on what the passenger ' +
  'asked.';
const MAX_ITERATIONS = 8;

interface McpToolDefinition {
  name: string;
  description?: string;
  inputSchema: {
    type: 'object';
    properties?: Record<string, unknown> | null;
    required?: string[] | null;
    [key: string]: unknown;
  };
}

function buildRunnableTools(
  tools: McpToolDefinition[],
  mcpClient: McpClient,
  onToolResult: (event: { type: 'tool-result'; toolName: string; structuredContent?: unknown }) => void,
): BetaRunnableTool[] {
  return tools.map((tool) => ({
    name: tool.name,
    description: tool.description ?? '',
    input_schema: tool.inputSchema as Anthropic.Beta.Messages.BetaTool['input_schema'],
    parse: (content: unknown) => content as Record<string, unknown>,
    run: async (args: Record<string, unknown>) => {
      const result = await mcpClient.callTool({ name: tool.name, arguments: args });
      if (result.structuredContent !== undefined) {
        onToolResult({ type: 'tool-result', toolName: tool.name, structuredContent: result.structuredContent });
      }
      const content = Array.isArray(result.content) ? result.content : [];
      const text = content
        .filter((block): block is { type: 'text'; text: string } => block.type === 'text')
        .map((block) => block.text)
        .join('\n');
      if (result.isError) {
        throw new Error(text || `${tool.name} failed`);
      }
      return text || '(no output)';
    },
  }));
}

export interface RunChatTurnOptions {
  /** Constructed by the caller with `dangerouslyAllowBrowser: true` and
   * the user's own key (frontend/lib/anthropicKey.ts) -- this module never
   * reads or constructs the key itself. */
  anthropic: Anthropic;
  model: string;
  mcpUrl: string;
  /** Drives StreamableHTTPClientTransport's own automatic reauth-on-401
   * (client/streamableHttp.js calling client/auth.js's `auth()`
   * internally) -- see BrowserMcpOAuthProvider (frontend/lib/mcpOAuthProvider.ts). */
  mcpAuthProvider: OAuthClientProvider;
  conversationHistory: Anthropic.Beta.Messages.BetaMessageParam[];
  userMessage: string;
}

export async function* runChatTurn(opts: RunChatTurnOptions): AsyncGenerator<ChatEvent> {
  const transport = new StreamableHTTPClientTransport(new URL(opts.mcpUrl), {
    authProvider: opts.mcpAuthProvider,
  });
  const mcpClient = new McpClient({ name: 'distant-signal-chat', version: '0.1.0' });
  await mcpClient.connect(transport);

  try {
    const { tools } = await mcpClient.listTools();

    const pendingToolResults: ChatEvent[] = [];
    const runnableTools = buildRunnableTools(tools as McpToolDefinition[], mcpClient, (event) => {
      pendingToolResults.push(event);
    });

    const runner = opts.anthropic.beta.messages.toolRunner({
      model: opts.model,
      max_tokens: 1024,
      system: SYSTEM_PROMPT,
      messages: [...opts.conversationHistory, { role: 'user', content: opts.userMessage }],
      tools: runnableTools,
      max_iterations: MAX_ITERATIONS,
      stream: true,
    });

    for await (const messageStream of runner) {
      for await (const streamEvent of messageStream) {
        if (streamEvent.type === 'content_block_delta' && streamEvent.delta.type === 'text_delta') {
          yield { type: 'text-delta', text: streamEvent.delta.text };
        }
      }
      while (pendingToolResults.length > 0) {
        yield pendingToolResults.shift()!;
      }
    }

    yield { type: 'done' };
  } finally {
    await mcpClient.close();
  }
}
```

- [ ] **Step 2: Write the failing test for `chatTurn.ts`**

```typescript
// frontend/lib/chatTurn.test.ts
import { describe, it, expect, vi } from 'vitest';
import Anthropic from '@anthropic-ai/sdk';
import { runChatTurn } from './chatTurn';

// Mocks the Anthropic SDK and MCP Client rather than hitting real network
// -- this loop's own control flow (drain text deltas, drain tool results
// between iterations, yield `done`) is what's under test here, not the
// real API integration (that's Task 11's Playwright coverage's job).
vi.mock('@modelcontextprotocol/sdk/client/index.js', () => ({
  Client: vi.fn().mockImplementation(() => ({
    connect: vi.fn(),
    listTools: vi.fn().mockResolvedValue({
      tools: [{ name: 'resolve_station', description: 'resolve a station', inputSchema: { type: 'object' } }],
    }),
    callTool: vi.fn().mockResolvedValue({ content: [{ type: 'text', text: 'York' }], structuredContent: { kind: 'station' } }),
    close: vi.fn(),
  })),
}));
vi.mock('@modelcontextprotocol/sdk/client/streamableHttp.js', () => ({
  StreamableHTTPClientTransport: vi.fn(),
}));

function fakeAnthropic(streamEvents: unknown[]): Anthropic {
  return {
    beta: {
      messages: {
        toolRunner: vi.fn().mockReturnValue(
          (async function* () {
            yield (async function* () {
              for (const event of streamEvents) yield event;
            })();
          })(),
        ),
      },
    },
  } as unknown as Anthropic;
}

describe('runChatTurn', () => {
  it('yields text-delta events for each text_delta stream event', async () => {
    const anthropic = fakeAnthropic([
      { type: 'content_block_delta', delta: { type: 'text_delta', text: 'Hello' } },
      { type: 'content_block_delta', delta: { type: 'text_delta', text: ' there' } },
    ]);
    const events = [];
    for await (const event of runChatTurn({
      anthropic,
      model: 'claude-x',
      mcpUrl: 'https://mcp.example.com/mcp',
      mcpAuthProvider: {} as never,
      conversationHistory: [],
      userMessage: 'hi',
    })) {
      events.push(event);
    }
    expect(events).toContainEqual({ type: 'text-delta', text: 'Hello' });
    expect(events).toContainEqual({ type: 'text-delta', text: ' there' });
    expect(events[events.length - 1]).toEqual({ type: 'done' });
  });

  it('ignores non-text_delta stream events', async () => {
    const anthropic = fakeAnthropic([{ type: 'content_block_delta', delta: { type: 'input_json_delta', partial_json: '{}' } }]);
    const events = [];
    for await (const event of runChatTurn({
      anthropic,
      model: 'claude-x',
      mcpUrl: 'https://mcp.example.com/mcp',
      mcpAuthProvider: {} as never,
      conversationHistory: [],
      userMessage: 'hi',
    })) {
      events.push(event);
    }
    expect(events).toEqual([{ type: 'done' }]);
  });
});
```

- [ ] **Step 3: Run to verify it fails, then passes**

```bash
cd frontend && npx vitest run lib/chatTurn.test.ts
```

Expected: FAIL first (module doesn't exist until Step 1's file is saved —
apply Steps 1 and 2 together, then run), then PASS.

- [ ] **Step 4: Rewrite `ChatPanel.tsx`**

Keep `parseSseFrames`, `asRenderedTrainLeg`, `ChatMessage`, and the
"track this leg" card rendering (unchanged shape — `ChatEvent`'s union is
identical to before, just produced by `runChatTurn` directly instead of
parsed off an SSE stream). Replace the `fetch('/api/chat', ...)` call and
the manual SSE-frame reading with a direct call to `runChatTurn`, gated on
having both an Anthropic key and a valid MCP token:

```tsx
// frontend/components/ChatPanel.tsx (key excerpt -- the rest of the file's
// message-list/input JSX, asRenderedTrainLeg, and ChatMessage/legs
// handling are UNCHANGED from the pre-refactor version; only the
// submission handler and the new gating below are new)
'use client';

import { useRef, useState, type FormEvent } from 'react';
import { Alert, Button, Card, Group, ScrollArea, Stack, Text, TextInput } from '@mantine/core';
import Anthropic from '@anthropic-ai/sdk';
import Link from 'next/link';
import type { RenderedTrainLeg } from '@/lib/types';
import { getAnthropicApiKey } from '@/lib/anthropicKey';
import { BrowserMcpOAuthProvider } from '@/lib/mcpOAuthProvider';
import { AnthropicKeySettings } from './AnthropicKeySettings';
import { runChatTurn, type ChatEvent } from '@/lib/chatTurn';

// ... ChatMessage, asRenderedTrainLeg unchanged from the pre-refactor file ...

const CHAT_MODEL = 'claude-opus-4-6';   // same unresearched-starting-figure posture as orchestrator/'s own model choice

type ChatError =
  | { kind: 'no-key' }
  | { kind: 'anthropic-rejected' }
  | { kind: 'mcp-reconnect' }
  | { kind: 'tool-error'; message: string };

export function ChatPanel() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [error, setError] = useState<ChatError | null>(null);
  const historyRef = useRef<Anthropic.Beta.Messages.BetaMessageParam[]>([]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    const trimmed = input.trim();
    if (!trimmed) return;
    setInput('');

    const apiKey = getAnthropicApiKey();
    if (!apiKey) {
      setError({ kind: 'no-key' });
      return;
    }

    const provider = new BrowserMcpOAuthProvider(`${window.location.origin}/chat/callback`);
    const tokens = provider.tokens();
    if (!tokens) {
      setError({ kind: 'mcp-reconnect' });
      return;
    }

    setMessages((prev) => [...prev, { role: 'user', content: trimmed, legs: [] }]);
    setMessages((prev) => [...prev, { role: 'assistant', content: '', legs: [] }]);
    setError(null);

    try {
      const anthropic = new Anthropic({ apiKey, dangerouslyAllowBrowser: true });
      let assistantText = '';
      const legs: RenderedTrainLeg[] = [];

      for await (const event of runChatTurn({
        anthropic,
        model: CHAT_MODEL,
        mcpUrl: `${process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL}/mcp`,
        mcpAuthProvider: provider,
        conversationHistory: historyRef.current,
        userMessage: trimmed,
      })) {
        applyChatEvent(event, { assistantText, legs, setMessages });
        if (event.type === 'text-delta') assistantText += event.text;
        if (event.type === 'tool-result') {
          const leg = asRenderedTrainLeg(event.structuredContent);
          if (leg) legs.push(leg);
        }
      }

      historyRef.current = [
        ...historyRef.current,
        { role: 'user', content: trimmed },
        { role: 'assistant', content: assistantText },
      ];
    } catch (err) {
      setMessages((prev) => prev.slice(0, -1));
      setError(classifyChatError(err));
    }
  }

  // ... rest of the render (message list, "track this leg" cards, input
  // form) UNCHANGED in structure from the pre-refactor file, plus:
  // - <AnthropicKeySettings /> rendered above the message list
  // - error.kind === 'no-key' | 'anthropic-rejected' | 'mcp-reconnect' | 'tool-error'
  //   each render a DISTINCT Alert, per Decision 6's three-error-classes requirement
}

function classifyChatError(err: unknown): ChatError {
  if (err instanceof Anthropic.APIError && err.status === 401) {
    return { kind: 'anthropic-rejected' };
  }
  const message = err instanceof Error ? err.message : 'Something went wrong.';
  if (/401|403|unauthoriz/i.test(message)) {
    return { kind: 'mcp-reconnect' };
  }
  return { kind: 'tool-error', message };
}
```

(`applyChatEvent`, the exact JSX for each of the four `ChatError` kinds,
and the unchanged message-list rendering are implementation-time detail
per the design doc's own "Explicitly out of scope" — the gating,
`classifyChatError`, and `runChatTurn` wiring above are the load-bearing
new logic this task must get right.)

- [ ] **Step 5: Rewrite `ChatPanel.test.tsx`**

Replace the old file's `fetch`-mocking tests with ones exercising the new
gating and error classification, mocking `runChatTurn` directly (its own
behavior is covered by `chatTurn.test.ts`, Step 2 — this file tests
`ChatPanel`'s own wiring around it):

```typescript
// frontend/components/ChatPanel.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ChatPanel } from './ChatPanel';
import { setAnthropicApiKey } from '@/lib/anthropicKey';

const mockRunChatTurn = vi.fn();
vi.mock('@/lib/chatTurn', () => ({
  runChatTurn: (...args: unknown[]) => mockRunChatTurn(...args),
}));

function seedMcpTokens() {
  localStorage.setItem('ds-mcp-oauth:tokens', JSON.stringify({ access_token: 'tok', token_type: 'Bearer' }));
}

describe('ChatPanel', () => {
  beforeEach(() => {
    localStorage.clear();
    mockRunChatTurn.mockReset();
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
  });

  it('shows a "no key" error when submitting without an Anthropic key set', async () => {
    seedMcpTokens();
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'when is the next train' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/set your anthropic api key/i)).toBeInTheDocument();
    expect(mockRunChatTurn).not.toHaveBeenCalled();
  });

  it('shows a "reconnect" error when no MCP token is stored', async () => {
    setAnthropicApiKey('sk-ant-test');
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'when is the next train' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/reconnect/i)).toBeInTheDocument();
    expect(mockRunChatTurn).not.toHaveBeenCalled();
  });

  it('renders streamed text-delta events as the assistant reply', async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        yield { type: 'text-delta', text: 'Next ' };
        yield { type: 'text-delta', text: 'train is at 10:15.' };
        yield { type: 'done' };
      })(),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'when is the next train' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/next train is at 10:15/i)).toBeInTheDocument();
  });

  it('renders a "track this leg" card for a plan_journey tool-result event', async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        yield {
          type: 'tool-result',
          toolName: 'plan_journey',
          structuredContent: { kind: 'train', uid: 'A12345', from: { crs: 'YRK' } },
        };
        yield { type: 'done' };
      })(),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'plan a trip' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByRole('link', { name: /track this leg/i })).toBeInTheDocument();
  });

  it('shows the Anthropic-key error distinctly from a tool error on a 401', async () => {
    setAnthropicApiKey('sk-ant-bad');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        throw Object.assign(new Error('invalid api key'), { status: 401, constructor: { name: 'APIError' } });
      })(),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/anthropic api key was rejected/i)).toBeInTheDocument();
  });

  it('does not submit an empty or whitespace-only message', () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    renderWithMantine(<ChatPanel />);
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(mockRunChatTurn).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 6: Run to verify all pass**

```bash
cd frontend && npx vitest run components/ChatPanel.test.tsx lib/chatTurn.test.ts
```

Expected: PASS. Iterate on Step 4's `ChatPanel.tsx` implementation
(`applyChatEvent`, the four distinct `Alert` renderings, message text
matching the exact strings these tests assert) until green — the tests
above are the actual specification for the remaining unwritten detail of
Step 4, not just a check on it.

- [ ] **Step 7: Run the full frontend suite + typecheck, commit**

```bash
cd frontend && npm test && npx tsc --noEmit && cd ..
git add frontend/lib/chatTurn.ts frontend/lib/chatTurn.test.ts frontend/components/ChatPanel.tsx frontend/components/ChatPanel.test.tsx
git commit -m "Rewrite ChatPanel: browser-side tool-calling loop, three distinct error states (Decisions 1, 6)"
```

---

## Task 11: Playwright e2e coverage for `/chat` (mocked network)

**Files:**
- Create: `frontend/e2e/chat.spec.ts`

**Interfaces:**
- Produces: a Playwright spec driving the real `/chat` page with mocked
  `page.route()` interception for both `api.anthropic.com` and
  `distant-signal-mcp`'s `/mcp`/`/token` endpoints, verifying the UI
  correctly renders a scripted streaming sequence and correctly surfaces
  each of Decision 6's three error classes — the concrete floor the design
  doc's own Decision 7/Testing section calls for, given there is no
  server-side integration point left to write a Vitest/Supertest test
  against.
- **Depends on:** Task 10 (the real `ChatPanel.tsx` this spec drives).

This is real, but narrower than the deleted `orchestrator/test/chat.test.ts`
suite, per Decision 7's own honest framing — named explicitly here, not
glossed over.

- [ ] **Step 1: Write the spec**

```typescript
// frontend/e2e/chat.spec.ts
import { test, expect } from '@playwright/test';

test.describe('/chat, mocked network', () => {
  test.beforeEach(async ({ page }) => {
    // Seed localStorage with a fake Anthropic key + MCP tokens before any
    // app JS runs, via an init script -- avoids re-driving the full OAuth
    // redirect chain for every case below, which is Decision 6's UX
    // trade-off (Open questions/risks #4), not this spec's own concern.
    await page.addInitScript(() => {
      window.localStorage.setItem('ds-anthropic-api-key', 'sk-ant-e2e-test');
      window.localStorage.setItem(
        'ds-mcp-oauth:tokens',
        JSON.stringify({ access_token: 'e2e-test-token', token_type: 'Bearer' }),
      );
    });
  });

  test('renders a streamed text reply from a mocked Anthropic response', async ({ page }) => {
    await page.route('**/mcp', async (route) => {
      const body = JSON.parse(route.request().postData() ?? '{}');
      if (body.method === 'tools/list') {
        await route.fulfill({ json: { jsonrpc: '2.0', id: body.id, result: { tools: [] } } });
        return;
      }
      await route.continue();
    });
    await page.route('**/v1/messages*', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'text/event-stream',
        body:
          'event: content_block_delta\ndata: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Next train is at 10:15."}}\n\n' +
          'event: message_stop\ndata: {"type":"message_stop"}\n\n',
      });
    });

    await page.goto('/chat');
    await page.getByPlaceholder(/ask about/i).fill('when is the next train');
    await page.getByRole('button', { name: /send/i }).click();
    await expect(page.getByText(/next train is at 10:15/i)).toBeVisible();
  });

  test('surfaces a distinct error when Anthropic rejects the key with a 401', async ({ page }) => {
    await page.route('**/mcp', (route) => route.continue());
    await page.route('**/v1/messages*', (route) =>
      route.fulfill({ status: 401, contentType: 'application/json', body: JSON.stringify({ error: { message: 'invalid x-api-key' } }) }),
    );

    await page.goto('/chat');
    await page.getByPlaceholder(/ask about/i).fill('hi');
    await page.getByRole('button', { name: /send/i }).click();
    await expect(page.getByText(/anthropic api key was rejected/i)).toBeVisible();
  });

  test('surfaces a distinct "reconnect" error on a 401/403 from /mcp', async ({ page }) => {
    await page.route('**/mcp', (route) => route.fulfill({ status: 401, contentType: 'application/json', body: '{}' }));

    await page.goto('/chat');
    await page.getByPlaceholder(/ask about/i).fill('hi');
    await page.getByRole('button', { name: /send/i }).click();
    await expect(page.getByText(/reconnect/i)).toBeVisible();
  });
});
```

(Exact mocked SSE payload shapes for Anthropic's real streaming format —
`event:`/`data:` framing, `message_start`/`content_block_start` events
this spec's minimal script above omits — should be verified against the
Anthropic TypeScript SDK's own streaming test fixtures at implementation
time; the sketch above establishes the mocking approach and the three
cases to cover, not a byte-exact wire fixture.)

- [ ] **Step 2: Run**

```bash
cd frontend && npx playwright test e2e/chat.spec.ts
```

Expected: PASS, 3 cases. If the app requires a running dev server per
`playwright.config.ts`'s existing `webServer` config, this happens
automatically (same as `e2e/service-worker.spec.ts`'s own setup, no new
Playwright config needed).

- [ ] **Step 3: Commit**

```bash
git add frontend/e2e/chat.spec.ts
git commit -m "Add Playwright e2e coverage for /chat: mocked Anthropic + distant-signal-mcp network"
```

---

## Task 12: Final cross-repo verification

**Files:** none (verification only)

**Interfaces:** none produced — this task confirms Tasks 1-11 compose
correctly end-to-end.

- [ ] **Step 1: This repo — full verification**

```bash
cargo check --workspace
cargo test -p api
cd frontend && npm test && npx tsc --noEmit && npx playwright test e2e/ && cd ..
helm template charts/distant-signal --set railMcp.enabled=true --set railMcp.publicUrl=https://example.com --set railMcp.frontendOrigin=https://example.com > /dev/null
grep -rn "orchestrator" charts/ .github/ docker-compose.yml *.env.example frontend/ crates/ | grep -vi "chatbot_allowed_users\|Task 5 of the client-side-tokens plan\|docs/superpowers" || echo "no stray orchestrator references"
```

- [ ] **Step 2: `distant-signal-mcp` — full verification**

```bash
cd /workspaces/distant-signal-mcp
npm run typecheck
npm test
grep -rn "orchestratorGrant\|ORCHESTRATOR_GRANT_TYPE\|X-Orchestrator-Internal-Token" src/ test/ || echo "no stray orchestrator-grant references"
```

- [ ] **Step 3: Manual end-to-end smoke test** (requires both services
  actually deployed against each other, not mocked — this step is honestly
  outside what any automated test in this plan covers, per the design
  doc's own Decision 7):

1. As an allowlisted user (`chatbot_allowed_users` row present), visit
   `/chat`.
2. Enter a real Anthropic API key via `AnthropicKeySettings`.
3. Send a message. Expect a redirect through `distant-signal-mcp`'s
   `/authorize` → the existing `/connect-claude/authorize` consent bridge
   → back to `/chat/callback` → `/chat`, then the message actually sends
   and a real streamed reply renders.
4. Confirm, via the browser's own Network tab, that the request to
   `api.anthropic.com` carries only the user's own key (never a Distant
   Signal-issued credential) and that no request to any Distant Signal
   server carries that key in any header or body.
5. Revoke the Anthropic key (rotate it in the Anthropic console) and
   confirm the "Anthropic API key was rejected" error state renders
   distinctly from a tool error.

- [ ] **Step 4: Confirm this plan's own open items are still accurately flagged**

Re-read the design doc's "Open questions / risks" section (5 items) —
none of them are resolved by this plan (by design; they were flagged as
out of this plan's scope). Confirm no task above silently attempted to
resolve one (e.g. no refresh-token grant was added, no rate-limiting for
per-user Anthropic spend was added) — if any was, that is scope creep to
back out, not a bonus.

---

## Testing

Covered per-task above. Net summary, restating the design doc's own
Decision 7 honesty: this plan deletes 18 `orchestrator/` tests (Task 5),
6 `frontend/app/api/chat/route.test.ts` tests (Task 5), and 12
`oauth-orchestrator-grant.test.ts` tests (Task 4) — 36 tests total, real
and already-passing before this plan runs. It adds: 6 unit tests
(`mcpOAuthProvider.test.ts`, Task 7), 4 (`callback/page.test.tsx`, Task
8), 7 (`anthropicKey.test.ts` + `AnthropicKeySettings.test.tsx`, Task 9),
2 (`chatTurn.test.ts`, Task 10), a rewritten `ChatPanel.test.tsx` (7 cases,
Task 10, replacing its own prior 7), 2 new CORS cases (Task 3), and 3
Playwright e2e cases (Task 11) — 31 new/rewritten test cases against 36
deleted. A real, acknowledged net reduction in mechanically-verifiable
coverage, in exchange for removing an entire server-side service and its
attack surface, exactly as the design doc's own Decision 7 frames the
trade-off.

## Not in this plan

- **The exact system prompt, model choice, or max-iteration bound** for
  the relocated tool-calling loop (`frontend/lib/chatTurn.ts`) — carried
  forward unresearched from `orchestrator/src/chat.ts`'s own starting
  figures, per the design doc's own "Explicitly out of scope."
- **A refresh-token grant** for `distant-signal-mcp`'s interactive
  `authorization_code` flow — unaffected by this plan, carried forward as
  the same open gap the shared-foundation plan already left.
- **Rate-limiting or abuse protection for a user's own Anthropic spend** —
  structurally out of Distant Signal's hands once the key and the calls
  are the user's own (design doc's own "Explicitly out of scope").
- **Migrating or backfilling `chatbot_allowed_users` rows** — an
  operator/provisioning concern, unaffected by Task 6's doc-comment-only
  change.
- **Removing/renaming the `chatbot_` prefix** on the retained table/route
  despite its re-framed meaning — cosmetic, explicitly flagged as
  out-of-scope by the design doc.
- **The NRE/Network-Rail-branding attribution question** for MCP
  tool-rendered output — unresolved, unaffected, carried forward unchanged.
- **Any change to `distant-signal-mcp`'s tool logic, `mcp-users`/
  `mcp-live-boards` access groups, or DCR/consent-bridge behavior** beyond
  Task 3's single additive CORS change — everything else about that
  authorization server is reused verbatim, per the design doc's Decision 2.

## Open questions / risks

Carried forward, unresolved, from the design doc's own "Open questions /
risks" section (all 5 items) — restated briefly here so an implementer
sees them without re-reading the whole design doc:

1. Anthropic's own CORS support for `dangerouslyAllowBrowser` could
   change/tighten in the future — a third-party dependency risk outside
   this repo's control, not mitigated by any task above.
2. Whether `BrowserMcpOAuthProvider`'s consumers (Tasks 8, 10) should lean
   further on `StreamableHTTPClientTransport`'s own built-in `authProvider`
   machinery vs. calling `client/auth.js`'s exported functions more
   directly is an implementation-time call — Task 10's sketch above uses
   the `authProvider` option; Task 8 calls `auth()` directly for the
   one-time code exchange. Both are viable per the design doc's own
   finding; revisit if either proves awkward against Next.js's own Client
   Component lifecycle during implementation.
3. Whether Decision 4's doc-comment re-framing (Task 6) is the only
   cosmetic follow-up needed, or whether other files reference the old
   "spend protection" framing not caught by this session's own grep —
   re-run `git grep -in "spend"` across `crates/api/` and
   `frontend/app/chat/` once Task 6 lands, to catch anything missed.
4. The genuine UX cost of removing `orchestrator/` (Task 5): a first-time
   user now completes a DCR + interactive-consent + code-exchange round
   trip before their first message, where the old orchestrator-grant
   design skipped this entirely for an allowlisted user. Not resolved by
   this plan — Task 11's Playwright suite's own `beforeEach` seeding is a
   testing convenience around this cost, not a fix for it.
5. Whether Anthropic's own per-key rate limits, multiplied across many
   users' own keys hitting `distant-signal-mcp`'s shared `/mcp` endpoint
   concurrently, interact with that adapter's existing `userLimiter`/
   `addressLimiter` in any surprising way — not load-tested by this plan;
   Task 12 Step 3's manual smoke test is a single-user check, not a load
   test.

## References

- `docs/superpowers/specs/2026-09-02-embedded-chatbot-option-b-client-side-tokens-design.md`
  — the spec this plan implements in full.
- `docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b.md` — the
  original plan whose 7 tasks are already implemented on the two branches
  Tasks 1-2 reconcile; cited throughout for exact prior file paths/shapes.
- `docs/superpowers/plans/2026-09-02-embedded-chatbot-shared-foundation-and-option-c.md`
  — unmodified by this plan; its own Tasks 1, 5, 7, 8 are the OAuth
  foundation this plan's Tasks 7-8 reuse as-is (DCR, `/authorize`,
  `/connect-claude/authorize`, `/token`).
- `docs/superpowers/plans/2026-09-02-mcp-server-oauth-access-groups.md` —
  the concurrent cross-repo work whose Task 7 (this repo's
  `railMcp.accessGroups`) and Task 9 (`distant-signal-mcp`'s
  `src/oauth/accessGroups.ts`) are what Tasks 1-2's own conflicts
  reconcile against; its own Task 9 structure (SEPARATE REPOSITORY
  framing, `cd /path/to/distant-signal-mcp` convention) is the template
  Tasks 2-4 of this plan follow.
- This session's own `git merge-tree` output against both repositories'
  actual current default branches — the concrete basis for Tasks 1-2's
  exact conflict locations and resolutions, re-verified fresh rather than
  taken from the design doc's own (slightly older) snapshot.
- `@modelcontextprotocol/sdk`'s installed `dist/esm/client/auth.d.ts`
  (`v1.29.0`, `/workspaces/distant-signal-mcp/node_modules/`) — the exact
  `OAuthClientProvider` interface Task 7 implements, read directly this
  session.
- `frontend/app/connect-claude/page.tsx`, `frontend/app/connect-claude/authorize/route.ts`
  — the existing `NEXT_PUBLIC_RAILMCP_PUBLIC_URL` precedent Task 8/10
  reuse directly, and the unmodified consent bridge Task 8's callback
  route is the redirect target for.
