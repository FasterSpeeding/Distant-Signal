# MCP Server First-Party Hosting — Design

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md`
(area-by-area structure, decisions with real alternatives weighed,
"Current relevant state" cited to real code/commits, "Open
questions/risks" as a first-class section) and reads directly on
`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`
(hereafter "the sibling doc") and
`docs/superpowers/specs/2026-09-01-train-mcp-integration-research.md`
(hereafter "the research doc") — both read in full this session, not
summarized from memory. No implementation plan is included; if this
document's recommendation is accepted, executing it is separate,
later work.

This is a decision document, not a migration plan. It investigates
whether `distant-signal-mcp` (the derived MCP server, currently living
in its own repository at `/workspaces/distant-signal-mcp`) should be
treated as a genuinely first-party Distant Signal ("DS") service, and
if so, what "first-party" should concretely mean here.

## Goal

Answer two separable questions, both raised by the task, with a real
recommendation for each:

1. **Code location.** Should `distant-signal-mcp`'s source move into
   this repo (a new top-level directory or workspace member, making
   this a monorepo) — or stay in its own repository?
2. **Fork lineage / ownership posture.** Independent of where the code
   lives: should `distant-signal-mcp` continue to be tracked as "a fork
   of `train-mcp` that DS customizes" (implying periodic
   upstream-merge maintenance and a ceiling on DS-specific divergence),
   or as "DS's own service that happened to bootstrap from
   `train-mcp`'s code" (implying DS stops tracking upstream entirely
   and just owns the code going forward)?

The sibling doc's own Decision 1 already answered a closely related
question once (see below) — this document's first job is to establish
precisely what that decision settled, what's changed since, and
whether either answer should now be revisited.

## Current relevant state (verified 2026-09-02)

### 1. The fork relationship, as it actually exists today — not as a git fork

`/workspaces/distant-signal-mcp`'s entire history is 14 commits, starting
from a single root commit:

```
feff6c2 Initial fork of train-mcp (unmodified baseline)   102 files changed, 65083 insertions(+)
f57fa88 Rename fork's advertised identity to distant-signal-mcp
30515e4 Add anonymous DsApiClient and DS_API_BASE_URL config (Decision 4)
32a9858 Migrate resolve_station onto DS's /public/stations, with a local tiploc/matchType shim (Decision 2)
435d1e5 Add pure leg-to-DS-line matching algorithm (Decision 3b)
7160c41 Add TTL-cached DS line catalogue (Decision 3d)
830fed5 Wire best-effort DS-sourced liveStatus annotation into plan_journey (Decision 3)
831f2ba Add Redis-backed OAuth stores + adapter config (Task 1)
122b5c1 Add the adapter's own OAuth server (Tasks 2-5)
d4ebcdc Gate /mcp behind the adapter's own bearer token; retire the Discord verifier (Task 7)
610d82d Make the /internal/* shared-secret comparison timing-safe
52b637d Merge chatbot-shared-foundation: OAuth 2.1 authorization server, replacing Discord auth
a42ea8c Add a non-interactive orchestrator-session grant to /token, for Option B
```

`git remote -v` in that repo returns **nothing** — there has never been
an `upstream` remote configured, and `git for-each-ref` shows only three
local branches (`master`, `access-groups-gating`,
`feat/embedded-chatbot-option-b`), no remote-tracking refs at all. The
root commit itself is a single 65,083-line insertion in one commit
covering 102 files — i.e. the original `train-mcp` codebase was
imported as a **wholesale copy** (from `train-mcp.zip`, the 2.7MB
archive sitting untracked at this repo's own root per the session's
`git status`), not via `git clone`/`git remote add upstream` followed
by history-preserving merges. **There is, and has only ever been, no
git-level fork relationship** — no shared commit ancestry with any
upstream repository, nothing to `git fetch upstream` from, nothing that
could silently drift out of sync with an upstream remote because no
such remote exists to drift from. "Fork" here is accurate only in the
informal, product sense (distant-signal-mcp's code derives from
train-mcp's code) — not in the technical git sense of a lineage DS
would need to periodically reconcile against.

This directly answers part of the task's Investigation Point 2: nothing
in this codebase relies on pulling upstream `train-mcp` updates,
because no mechanism to do so exists or ever existed. The "periodic
upstream-merge maintenance burden" that a real git fork would carry is
not a cost DS is currently paying, has ever paid, or would newly incur
by declaring the fork relationship over — there is nothing to
formally sever. What DS *is* carrying is git blame/history noise:
every file inherited from the original copy-in still shows `feff6c2`
as its origin commit, and 65,083 of the repo's current lines (the vast
majority of its ~68,000-line total, per the diff-stat below) are still
byte-for-byte or near-byte-for-byte `train-mcp` code, never rewritten,
never re-attributed.

### 2. How much has actually diverged, by volume

```
$ git diff --shortstat feff6c2 HEAD
38 files changed, 2803 insertions(+), 901 deletions(-)
```

Net ~1,900 lines changed across 13 commits, against a 65,083-line base.
By raw line count, `distant-signal-mcp` is still overwhelmingly
`train-mcp`'s own code — the CIF timetable ingester, the RAPTOR/CSA
journey planner, the LDBWS client/mapper, the board-rendering layer are
all untouched. But the ~1,900 changed lines are concentrated exactly
where DS-specific work has happened: the entire auth layer was replaced
(`test/discord-auth.test.ts` deleted outright, 335 lines; a new
`src/oauth/*` adapter and its own OAuth 2.1 authorization-server tests
added), `resolve_station` was rewired onto DS's own `/public/stations`,
and `plan_journey` gained a DS-line-catalogue annotation step with its
own matching algorithm and cache. This is the shape the research doc
predicted for "shape B" (see below): a service that keeps train-mcp's
expensive, already-tested planner/ingest investment untouched while
swapping the surface DS actually needed to own.

### 3. The original reasoning for forking rather than building from scratch

The research doc's own "Sketch: where would a derived service live"
section (`2026-09-01-train-mcp-integration-research.md:310-357`) framed
two shapes: (A) a new Rust crate calling `crates/api/src/data/*`
directly, or (B) keep it a separate TypeScript service, close to
train-mcp's existing architecture, swapping only what DS's API could
serve. It rejected (A) explicitly because "none of train-mcp's
substantial existing timetable/planner code (the expensive part... is
reusable in Rust without a full rewrite," and chose (B) because it
"keeps train-mcp's already-substantial, already-tested CIF/planner
investment intact rather than discarding it."

The sibling design doc's Decision 1
(`2026-09-01-train-mcp-integration-design.md:271-337`) restates this
more sharply: rewriting `find_services`/`plan_journey` in Rust would
duplicate "a multi-week Phase 2a/2b investment, already done once" — a
26,848-schedule, 316,362-calling-point engine, per the research doc's
own measurement. **The original reasoning was squarely "save the
multi-week cost of re-implementing a working timetable/journey-planner
engine," not any broader judgment about repo topology, licensing
posture, or long-term maintenance strategy.** Decision 1 goes on to
make an *explicit* code-location call, separate from the
shape-of-service call: "Where the derived service's code lives is a
separate question from where its deployment lives, and the two get
different answers" — code stays in its own repository; deployment
(Helm subchart, compose service) lives in this repo, `import[ing] an
entire second language's tests, tooling, and CI matrix... into a
Rust-workspace-centric repo whose own CI already spans Rust and one
Next.js app would grow this repo's own build surface for a service
this repo's CI doesn't need to build."

This document's job is to determine whether that specific tradeoff
still holds five months (in-story) and roughly 20 substantial commits
later — not to re-derive it from nothing.

### 4. What's already first-party, independent of code location

Every deployment-facing surface already treats `distant-signal-mcp` as
a normal, first-class part of this repo's own deployment story — this
was true even under Decision 1's own framing ("first-class here means
*this repo's deployment story includes and manages it*"), and remains
true, unchanged, through everything that's landed since:

- **Helm chart.** `charts/distant-signal/templates/railmcp-deployment.yaml`
  and `railmcp-service.yaml` follow the exact same conventions as every
  other component in the chart: `distant-signal.labels`/
  `distant-signal.selectorLabels` helpers, the same
  `distant-signal.podSecurityContext`/`containerSecurityContext`
  helpers, the same `serviceAccountName`/`imagePullSecrets`/
  `resources`/`nodeSelector`/`tolerations`/`affinity` value wiring every
  other Deployment in this chart uses. `ingress.yaml:8-13,58-69` gives
  it its own `ingress.railMcp.{enabled,host}` rule, parallel to
  `ingress.api`'s, including the same `fail` guard pattern
  (`ingress.railMcp.enabled` requires both a host and
  `railMcp.enabled`).
- **Secrets.** `secret.yaml` and `_helpers.tpl` render its credentials
  (LDBWS product keys, the OAuth internal-completion token) through the
  same `existingSecret`/`existingSecretXKey` pattern (per
  `_helpers.tpl:472-482`'s own header comment) every other credential
  in this chart follows — no separate credential-management story.
- **Auth.** As of `52b637d`/the mcp-server-oauth-access-groups design,
  `distant-signal-mcp` no longer authenticates end users via Discord at
  all — it is its own OAuth 2.1 authorization server backed by DS's
  own, unmodified OIDC login (`railmcp-deployment.yaml:83-86`'s comment:
  "replaces the DISCORD_CLIENT_ID/DISCORD_ALLOWED_USER_IDS env vars this
  block used to render here"), and gates tool access on DS's own
  Authentik-native access groups (`MCP_USERS_GROUP`/
  `MCP_LIVE_BOARDS_GROUP`, added `cc27754`/`b105b36`). A DS user reaches
  it through the exact same identity provider as the rest of DS.
- **docker-compose.yml.** `rail-mcp` (`docker-compose.yml:572-612`) is
  gated behind `profiles: ["rail-mcp"]` the same opt-in mechanism as
  every genuinely-optional component, sits alongside every other DS
  service in the same file, and is explicitly compared in its own
  comment to `schedule-sftp`'s externally-built-image pattern — the
  established precedent in this exact file for "a service this repo
  deploys but doesn't build."
- **Branding.** No Discord/upstream branding survives anywhere in the
  *deployed* surface. It does, however, survive in the **source repo
  itself**, not yet cleaned up: `distant-signal-mcp/README.md` still
  opens with `# train-mcp` (not `distant-signal-mcp`) and its
  Authentication section still fully documents `DISCORD_CLIENT_ID`/
  `DISCORD_ALLOWED_USER_IDS`/the Discord redirect-URI setup steps
  (`README.md:110-116,147-148,162-178,274-275`), and `TODO.md` still
  opens `# train-mcp — TODO` and lists "Discord application" as an
  outstanding setup step. Both are now **stale relative to the code**:
  the OAuth adapter replacing Discord auth landed in commits
  `831f2ba`..`52b637d`, but neither doc was updated afterward. This is
  real, if minor, evidence that ownership/branding hygiene inside the
  MCP repo itself lags behind what's already true operationally — an
  argument that documentation discipline, not repo location, is the
  actual gap.

**Conclusion: deployment, secrets management, authentication, and
authorization are already fully first-party in every practical sense.**
The only things that are not first-party today are (a) the source code
repository's physical location, and (b) the MCP repo's own
self-description (README/TODO still narrate it as `train-mcp` with
Discord auth, despite the code no longer matching).

### 5. Language/tooling reality, and what a monorepo move would and wouldn't unify

This repo's backend is Rust (`crates/aggregator`, `crates/api`,
`crates/common`, `crates/enricher`, `crates/notifier`,
`crates/poller-incidents`, `crates/poller-ldbws`,
`crates/poller-stations`, `crates/poller-tfl`, `crates/poller-tocs`,
`crates/schedule-ingest`, `crates/schedule-reference`,
`crates/trust-consumer` — 12 crates), its frontend is Next.js/React
(`frontend/`), and CI (`.github/workflows/ci.yml`,
`.github/workflows/containers.yml`) builds and tests exactly those two
ecosystems — neither workflow file references `mcp` or
`distant-signal-mcp` at all (confirmed by grep). `distant-signal-mcp`
is TypeScript/Node (`package.json`'s `"engines".node: ">=24"`, `tsc`/
`vitest`/`tsx` toolchain, `@modelcontextprotocol/sdk` dependency) — a
third, wholly separate language and build ecosystem from either half of
this repo.

**A code move into this repo would not unify the tech stack — it would
make this repo host three ecosystems instead of two, in either
arrangement.** What it *would* concretely buy:

- **One `git log`/one PR for a change that touches both sides.** This is
  real, not hypothetical: `charts/distant-signal/templates/`,
  `docker-compose.yml`, and `values.yaml` already live in this repo and
  already reference `distant-signal-mcp`-specific env vars/behavior
  (`TRUST_PROXY_HOPS`, `MCP_USERS_GROUP`, `OAUTH_INTERNAL_COMPLETE_TOKEN`,
  etc.) that only make sense in light of what the *other* repo's code
  actually does.
- **Simpler cross-repo coordination**, evidenced concretely by this
  exact session (below), not speculatively.

What it would **not** buy: a shared build toolchain, a shared test
runner, a shared linter/formatter config, or a shared release/versioning
scheme — `cargo`/`npm` remain two different worlds inside one
repository exactly as much as they are two different worlds across two
repositories. `schedule-ingest`'s own Docker build
(`docker/schedule-ingest.Dockerfile`) already coexists with the Next.js
frontend's build inside this one repo's `containers.yml`, so a third
independent build definition (`distant-signal-mcp`'s own
`Dockerfile`/`tsc`) is not itself a novel kind of complexity for this
repo's CI to host — but it is still a fully separate pipeline, monorepo
or not.

### 6. Real, quantified cross-repo coordination friction this session

The task's premise — that today's session had to explicitly coordinate
branch/commit state across the two repos, repeatedly — is directly
confirmed by this repo's own commit messages, not inferred:

- **`b105b36`** ("Add railMcp.accessGroups.\* chart values (Discord
  retirement already landed on main)"): *"Task 7 of the
  mcp-server-oauth-access-groups plan originally also retired
  railMcp.discord.\* and its secret.yaml/_helpers.tpl plumbing, but that
  retirement already landed on main via commit 1bc3603... confirmed by
  re-checking this session — no discord.\* values, secret keys, or
  helpers remain anywhere in this chart."*
- **`cc27754`** ("Add MCP_USERS_GROUP/MCP_LIVE_BOARDS_GROUP env vars to
  rail-mcp (Discord retirement already landed on main)"): the same
  pattern repeats one commit later — a second plan task (Task 8) also
  had to be re-derived against state that had already landed, requiring
  an explicit re-check rather than being visible from a single
  `git log`.
- **`621541e`** ("Fix railmcp per-address rate limiter collapsing
  behind Ingress: set TRUST_PROXY_HOPS"): a bug that could only be
  diagnosed by reading `distant-signal-mcp`'s own `src/app.ts`/
  `src/config.ts` behavior (how its `addressLimiter` resolves `req.ip`)
  and then fixing it from *this* repo's chart — a single logical fix
  that spans both repos' source, landing as two separate,
  independently-timed commits in two separate histories.
- **`7643b5f`** ("Corrections: reverse train-mcp Decisions 4/6 to
  public + DS-OIDC-gated") and **`3becd14`** ("Corrections: re-evaluate
  embedded-chatbot MCP research now that distant-signal-mcp is going
  public") show the same pattern one level up: a design decision made
  in one doc had to be explicitly revisited once state in the *other*
  repo changed underneath it.

**This is a real, recurring cost, not a one-off.** At minimum four
distinct points in this session's own history required someone to stop,
notice that state assumed-not-yet-landed had actually already landed in
the sibling repo, and explicitly reconcile before continuing — twice
inside a single implementation plan's own task list (Tasks 7 and 8 of
the same plan). A single `git log` in one repository would have shown
this automatically; two repositories required either memory of what had
shipped where, or an explicit re-check, both of which failed at least
once here (the plan's own task list was written assuming work not yet
done, then found already done).

### 7. Precedent for multi-language/monorepo-vs-polyrepo decisions in this repo

`distant-signal-mcp` is not the first non-Rust/non-Next.js component in
this deployment story, but every existing precedent is a **third-party
upstream project this repo genuinely doesn't own the source of**, not a
DS-authored service in a foreign language:

- **Authentik** (`devauthentik-server-deployment.yaml`, etc.) — an
  entirely separate open-source identity-provider project
  (`ghcr.io/goauthentik/server`), referenced by the sibling doc's own
  Decision 1 as "the closest existing precedent for 'a service this
  deployment depends on but doesn't own the source of.'"
- **sftpgo** (`schedule-sftp`, per `docker-compose.yml`'s own comment
  precedent cited for `rail-mcp`) — likewise a genuine third-party
  image this repo configures but does not author.

`schedule-ingest`/`schedule-reference` (the DS-authored half of the
`scheduleFeed` pairing) are Rust — i.e. every DS-*authored* non-frontend
service in this chart today is a Rust crate in `crates/`, with the sole
exception of `distant-signal-mcp`. **`distant-signal-mcp` would be the
first case of DS-authored-but-not-Rust code in this deployment, and the
Helm chart's own comments already say so explicitly**:
`railmcp-deployment.yaml:41-50`'s security-context comment states this
plainly — "This is a third-party-built image (from this chart's own
perspective — it is this repo's derived service, but not built by this
repo's CI)... same readOnlyRootFilesystem: false stance
devauthentik-server-deployment.yaml takes for
ghcr.io/goauthentik/server, for the same reason." **The chart is
already treating a DS-authored service as if it were an
externally-maintained third party**, purely because of where its code
happens to live — this is the clearest single piece of evidence that
code location and ownership posture have become coupled in a way that
doesn't reflect reality: `distant-signal-mcp` gets a weaker security
posture (no filesystem verification) not because DS doesn't control the
code, but because this repo's own CI doesn't build it.

There is one further, very recent data point: the
`2026-09-02-embedded-chatbot-dual-mode-design.md` doc's Decision 2
(lines 296-335) designs a **third** DS-authored TypeScript service (a
chat "orchestrator," Anthropic SDK tool-calling loop) and explicitly
decides it should be its own deployed process, `ClusterIP`-only,
**separate from `distant-signal-mcp`** — but does not settle, anywhere
in that document, which *repository* its source should live in. As of
this investigation the orchestrator does not exist yet (no
`orchestrator/` directory in either repo) — it remains a design, not
shipped code, so it's a live, near-term instance of exactly the
question this document is answering, not settled precedent either way.

## Decisions

### 1. Code location: move now, or keep the two-repo split? — **Recommend: keep the split, revisit only if a third DS-authored TypeScript service materializes**

**Chosen: no code move at this time.** Three genuine alternatives were
weighed:

- **(A) Move `distant-signal-mcp`'s source into this repo now** (e.g.
  `mcp/` or `services/rail-mcp/` at top level), making this a
  three-ecosystem monorepo (Rust + Next.js + Node/TS).
- **(B) Leave the code split exactly as Decision 1 originally set it —
  code in its own repo, deployment/chart/compose in this repo.**
  **Chosen.**
- **(C) Move the code, but only later, once/if a second DS-authored
  TypeScript service (the orchestrator, or a future one) actually
  ships** — deferred, not chosen now, but named because it changes the
  cost/benefit math materially (see Migration cost/risk, below).

Reasoning: **the concrete benefit a move buys — fewer git-log blind
spots, one PR for a cross-repo change — is real (Current relevant state
§6 above) but is a coordination-overhead problem, and coordination
overhead is exactly what a monorepo trades against build/CI/ownership
complexity it doesn't remove.** A move would not resolve the actual
root cause of §6's friction: none of those four coordination incidents
happened because the code lived in a different *repository* per se —
they happened because a plan/design was written assuming work state
that had, in fact, already shipped somewhere the author wasn't
currently looking. That's a process/communication gap (a plan's own
task list going stale against reality), and a monorepo narrows where
"reality" could be hiding, but doesn't eliminate the underlying
failure mode — a single large repo can just as easily have a stale
plan referencing a merged PR in a different directory. What a monorepo
*would* concretely fix is exactly one of the four incidents (§6, third
bullet: the `TRUST_PROXY_HOPS` diagnosis spanning both repos' source)
— genuinely easier with one `git grep`/one checkout instead of two.
That is a real but narrow win, not proportionate on its own to a
repository restructure, on top of the extra cost this move would
impose on this repo's own CI surface (see Migration cost/risk).

The Helm chart's own `readOnlyRootFilesystem: false` treatment of
`distant-signal-mcp` (Current relevant state §7) is real evidence that
ownership perception is currently mis-set — but that's a §2 (fork
lineage) problem, not a §1 (code location) one: the chart's own comment
gives the *security-verification* reason ("not built by this repo's
CI"), not a branding reason, and that reason would remain true even
after declaring the fork lineage over, unless the source physically
moves into this repo's own CI-built surface. **This is the one place
where declining to move the code has a real, ongoing cost**: as long as
`distant-signal-mcp`'s image is built externally and referenced by
`repository`/`tag` in `values.yaml` (Decision 1's own chosen shape,
unchanged), this repo's own CI cannot verify what's actually in that
image, and the chart's security posture for it will keep reflecting
that (weaker `securityContext`, no build-time scanning this repo's own
`containers.yml` would otherwise provide). This is worth carrying
forward as a named, accepted tradeoff rather than an oversight — see
Open questions/risks.

**When this recommendation should be revisited:** if the orchestrator
service (embedded-chatbot-dual-mode-design.md's Decision 2, not yet
built) or any future DS-authored TypeScript service ships as a second
Node/TS component in this deployment, the "importing one language's
tooling costs this much" argument in Decision 1 weakens on a
per-service basis — two or more DS-authored TS services sharing one
external CI/build setup starts to look like duplicated infrastructure
regardless of where either one's source lives, and a shared
`mcp-services/` or similar grouping (still possibly its own repository,
but now clearly justified as one) becomes a more proportionate
conversation. That trigger has not yet occurred (orchestrator is
design-only today), so it's named as a revisit condition, not acted on.

### 2. Fork lineage / ownership posture: **Recommend: formally declare the fork lineage over — DS's own service, bootstrapped from `train-mcp`, no upstream tracking — independent of the code-location answer**

**Chosen: stop describing/treating `distant-signal-mcp` as "a fork of
`train-mcp`" and start describing it as "Distant Signal's own MCP
service, originally bootstrapped from `train-mcp`'s code."** This is
the answer to the second question the task asks, and it is
**independent** of Decision 1 above — a repo can be fully DS-owned in
posture while its code still lives in a separate repository (the
Authentik/sftpgo precedent is the *opposite* combination: separately
owned, but deployment-managed here).

Reasoning:

- **There is no real fork lineage to sever.** Current relevant state §1
  already establishes this is not a git fork at all — no `upstream`
  remote, no shared commit ancestry, a single wholesale copy-in commit.
  Declaring the fork "over" costs nothing mechanically; there is no
  `git fetch upstream && git merge` workflow anyone is relying on or
  would need to stop doing.
- **The original reasoning for forking (§3) was purely "reuse the
  timetable/planner engine to save multi-week reimplementation cost,"
  not "stay aligned with an actively-maintained upstream."** Nothing in
  either design doc frames ongoing upstream tracking as a goal — the
  research doc never asks whether `train-mcp` (the original project) is
  even still maintained, and the sibling doc's Decision 1 treats the
  copy as a one-time transplant of working code, not an ongoing feed.
  There is no evidence anywhere in either repo's history that upstream
  `train-mcp` updates were ever pulled, checked for, or intended to be
  pulled after the initial copy.
- **By volume, most of the code is still literally `train-mcp`'s own
  code** (§2: ~65K of ~68K lines untouched) — but this argues *for*,
  not against, declaring DS ownership: an untracked, un-reconcilable
  65K-line base that nobody is comparing against a moving upstream
  target is exactly the situation where continuing to call it "a fork"
  invites false expectations (that there's a diff to review, an
  upstream to check for security fixes against, etc.) that don't
  correspond to anything real happening in practice.
- **The one place lineage-as-posture is currently doing real, visible
  harm** is the Helm chart's own security-context comment (§7): it
  treats `distant-signal-mcp` as if it were literally
  `ghcr.io/goauthentik/server` — a genuine third party — for
  verification purposes. That comment's *reasoning* ("not built by this
  repo's CI") stays true regardless of this decision, since it's about
  where the image is *built*, not who *owns* the source — but the
  *framing* ("third-party-built image... it is this repo's derived
  service, but not built by this repo's CI") already gets this half
  right and should be the template: own the code, note the CI gap
  honestly, don't conflate the two.
- **README/TODO staleness (§4) is a direct, cheap, immediate action
  item regardless of this decision**: `# train-mcp` as a title and a
  fully Discord-documented Authentication section, in a repo whose code
  has not used Discord auth since `52b637d`, is stale documentation, not
  an ownership-posture question — but leaving it unfixed actively
  undermines a "this is DS's own service now" posture with every reader
  who opens the README.

**What this recommendation does *not* imply**: it does not require or
recommend re-licensing, re-branding every internal comment
(`src/oauth/provider.ts`'s references to "train-mcp design's Decision
4d" as design-doc citations are fine — they're historical/provenance
comments, not branding), or discarding the git history that documents
where the code came from. It also does not resolve the licensing
question the sibling doc already flagged and left open (Licensing
note, `train-mcp-integration-design.md:1171-1225`): no LICENSE file has
ever existed anywhere in this repo's history (confirmed:
`git log --all --diff-filter=A --name-only | grep -i license` returns
nothing), for either the original `train-mcp` code or anything added
since — a real open item, but a legal one, not resolved by an ownership
posture decision either way. See Open questions/risks.

## Architecture

No code move is recommended (Decision 1), so no new repo layout is
being proposed. For completeness, the shape a future move *would* take
if the Decision 1 revisit trigger (a second DS-authored TypeScript
service) occurs is sketched at a high level only — this is explicitly
not a plan:

- A new top-level directory (e.g. `mcp-services/` or similar, name TBD)
  holding one subdirectory per DS-authored TypeScript service
  (`mcp-services/rail-mcp/`, `mcp-services/orchestrator/` if/when it
  ships), each keeping its own `package.json`/`tsconfig`/test runner —
  not merged into one Node workspace unless a concrete reason to share
  dependencies between them emerges.
- `.github/workflows/` gains a new, independent job (not folded into
  the existing Rust/Next.js jobs) building/testing whatever lives under
  that directory — mirroring how `ci.yml` presumably already separates
  Rust and frontend jobs today (not fully audited this pass; assumed
  from the two-ecosystem split already established).
- `docker/`'s existing per-service Dockerfile convention
  (`docker/schedule-ingest.Dockerfile`, etc.) extends with a
  `docker/rail-mcp.Dockerfile`, replacing the current externally-built-
  image reference in `values.yaml`/`docker-compose.yml` with a
  same-repo `build:` context, matching how `crates/*` services are
  already built by this repo's own CI rather than referenced by
  external tag.
- `charts/distant-signal/templates/railmcp-*.yaml` lose their
  "third-party-built image" security-context caveat (§7) once the image
  is actually built and scannable by this repo's own
  `containers.yml`.

This sketch is included only to make Decision 1's revisit condition
concrete, not as a proposal to act on now.

## Migration cost/risk assessment

Even under the "move only if triggered" framing, it's worth being
honest about what a move would cost, since the task specifically asks
not to default to "just do it."

- **History preservation.** `distant-signal-mcp`'s history is small (14
  commits) and self-contained — `git subtree add`/`git filter-repo`
  either would work cleanly; this is genuinely low-risk and low-effort
  relative to a typical monorepo migration, *because* the repo is young
  and small. This is the one place the "it's cheap, do it" instinct is
  actually well-supported by the evidence — if a move is ever done, the
  history-migration mechanics themselves are not the hard part.
- **CI pipeline.** Real, non-trivial: a new independent Node/TS build
  job needs to be added to `.github/workflows/ci.yml`/`containers.yml`
  from scratch (today's workflows have zero references to `mcp` or
  Node tooling, confirmed by grep) — test runner (`vitest`), typecheck
  (`tsc --noEmit` × 2, per `package.json`'s own `typecheck` script),
  and a new container build stage all need to be authored, not merely
  copied from an existing job.
- **Docker build context.** `distant-signal-mcp`'s own build (its
  `Dockerfile`, whatever it currently is — not inspected this pass) has
  to be re-pointed at a new build context inside this repo, and
  `docker-compose.yml`'s `rail-mcp` entry has to switch from an
  externally-tagged `image:` reference to a `build:` context, changing
  local-dev iteration mechanics (image pulls vs. local builds) for
  anyone currently using the compose-profile workflow.
- **Helm chart paths.** Low risk — the chart already references
  `distant-signal-mcp` purely by rendered image string
  (`repository`/`tag` in `values.yaml`), not by any path into the
  source repo, so a code move doesn't require touching chart template
  logic at all, only the CI step that produces the image reference.
- **Timing/velocity risk.** This session has active, uncommitted-to-main
  development happening in `distant-signal-mcp` right now (three
  branches: `master`, `access-groups-gating`,
  `feat/embedded-chatbot-option-b` — the latter two not yet merged to
  `master` per the branch listing in §1) and a possible near-term
  orchestrator service still at design stage. **A migration mid-flight,
  while active feature branches exist in the source repo, is
  meaningfully riskier and more disruptive than one done at a natural
  pause point** — this is a real argument for "not now" independent of
  the underlying merits, and reinforces Decision 1's "wait for a
  concrete trigger" framing rather than doing this opportunistically.

**Overall: cheap on the git-mechanics axis, real but bounded cost on
the CI/build-pipeline axis, and genuinely risky on timing given active
in-flight branches.** This is not a "just do it, it's obviously better"
case — it's a real tradeoff that isn't currently forced by anything,
which is why Decision 1 recommends deferring it to a concrete trigger
rather than executing it speculatively.

## Explicitly out of scope

- **A full migration plan or implementation for either decision.** If
  this document's Decision 1 is later reversed (the orchestrator or
  another DS-authored TS service ships), the actual `git subtree`/CI/
  Dockerfile/chart work is separate, future planning-and-execution
  work, not sketched further than the high-level Architecture section
  above.
- **Fixing the stale README/TODO branding in `distant-signal-mcp`.**
  Flagged as a genuine, cheap, low-risk action item in Decision 2, but
  it's a documentation edit in the *other* repository, not a design
  decision, and this task's constraints are design/investigation only,
  no code changes to either repo.
- **The NRE/Darwin data-licensing "presentation" question** the sibling
  doc already flagged and left open
  (`train-mcp-integration-design.md`'s own Licensing note). Unaffected
  by either decision in this document — it's about what the *service*
  serves to end users, not where its code lives or how its lineage is
  described.
- **Whether/where the chat orchestrator's source should live.** Named
  in Current relevant state §7 and Decision 1's revisit condition as
  the concrete future trigger, but the orchestrator itself is
  design-only as of this investigation (embedded-chatbot-dual-mode-
  design.md's Decision 2) — resolving its own repo placement is that
  service's own future design decision, informed by but not settled by
  this document.
- **Re-auditing `distant-signal-mcp`'s actual `Dockerfile`/build
  process.** Referenced in Migration cost/risk by inference from
  `docker-compose.yml`'s comment describing it as "externally-built,"
  not independently opened and read this pass.

## Open questions/risks

1. **Original `train-mcp` project's own licence terms were never
   established, by either this document or the sibling doc.** No
   LICENSE file exists anywhere in `distant-signal-mcp`'s git history
   (confirmed by full-history grep), and the sibling doc's own
   Licensing note addresses only the *NRE data* licensing question, not
   whatever licence (if any) the original `train-mcp` project itself
   was released under. Declaring the fork lineage "over" (Decision 2)
   does not resolve whatever obligations, if any, attach to code
   originally copied from a differently-licensed (or unlicensed)
   upstream project — this is a genuine legal open question this
   document cannot close, flagged here rather than assumed away.
2. **This document assumes the orchestrator stays undecided on repo
   placement** — if work on it has already resumed or concluded
   somewhere since this investigation (the task's own framing flags
   this as timing-dependent), Decision 1's revisit trigger may already
   be closer to firing than this document assumes, and should be
   re-checked against the orchestrator's actual current state before
   being treated as still-open.
3. **The `readOnlyRootFilesystem: false`/unverified-image security
   posture (§7) is accepted, not fixed, by this document's
   recommendation.** As long as Decision 1 holds (no code move), this
   repo's own CI genuinely cannot verify `distant-signal-mcp`'s image
   contents, and that gap is real regardless of how its ownership is
   described. Whether that residual risk is acceptable at DS's current
   scale is a judgment call this document surfaces but does not itself
   make.
4. **This document did not independently audit `distant-signal-mcp`'s
   own CI/test setup** (whether it has its own GitHub Actions, how it's
   currently built into an image at all) — only its `package.json`
   scripts and the DS-side chart/compose references to it. If
   `distant-signal-mcp` has no CI of its own today, that's an
   independent, more urgent gap than this document's repo-location
   question, worth checking directly.
5. **"Declare the fork lineage over" is a posture/documentation
   change with no enforcement mechanism proposed here.** Nothing stops
   a future session from re-introducing "let's check what upstream
   train-mcp has done since" as a task; this document recommends
   against that framing but doesn't design any guardrail (e.g. a
   README statement, a CLAUDE.md note in the MCP repo) to make the
   posture durable. A concrete, cheap follow-up worth naming: add a
   short "this is DS's own service, not tracking any upstream" note to
   `distant-signal-mcp`'s own README when its stale Discord content is
   corrected (Decision 2's flagged action item) — not designed further
   here since it's a documentation edit, not a decision.
