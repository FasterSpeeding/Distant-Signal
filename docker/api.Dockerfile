# syntax=docker/dockerfile:1@sha256:ecfaec9ed6d810b56388c508f4121597bfbba70d41a6dfeee4d8cad5f295fc32
# Multi-stage build for the `api` service.
#
# Builder pin: edition 2024 (used by every crate in this workspace) needs
# rustc 1.85+, and the five poller Dockerfiles also pin 1.88 because their
# resolved Cargo.lock pulls in transitive icu_* deps (via reqwest -> url ->
# idna -> idna_adapter -> icu_normalizer/icu_provider) needing 1.88+. `api`
# hits that same icu_* chain too (`cargo tree -p api -i icu_provider` shows
# it arriving via oauth2/redis/reqwest/sqlx-core, all of which share the
# workspace's single resolved `url` version) — but even without that, it
# independently needs 1.88+ because it pulls in `sqlx-postgres`, whose
# transitive `home` crate (pinned to 0.5.12 in the workspace Cargo.lock)
# requires rustc 1.88+ — confirmed by actually building this image against
# rust:1.86-bookworm first and hitting:
#   "error: rustc 1.86.0 is not supported ... home@0.5.12 requires rustc 1.88"
# 1.88 is the real floor for *this* crate's dependency tree, and it turns
# out to be the workspace-wide floor too — every service's Cargo.lock
# resolves to the same rustc 1.88 requirement by way of the icu_* chain
# above (api additionally hits it via `home`).
#
# This image carries SIX binaries: `api` (the ENTRYPOINT), `corpus_compare`
# (a read-only CORPUS report, see crates/api/src/bin/corpus_compare.rs),
# `backfill_trains`, the one-off, idempotent shared-train-identity backfill
# that MUST be run before this image is first started against a database
# with pre-existing `tracked_trains` data, and `backfill_incident_lines`,
# the one-off, idempotent `incidents.affected_lines` backfill (see
# docs/incident-affected-lines-backfill.md -- optional, but the incident
# archive's Line filter returns nothing for pre-existing rows until it has
# run), `replay_uidless_movements` (one-off, idempotent, see below) and
# `backfill_line_train_summaries` (optional, idempotent, see below).
# `api`'s own startup enforces that
# ordering (it refuses to apply `20260906140000_drop_legacy_columns.sql`
# while unbackfilled rows remain), so shipping both here is what makes the
# enforced sequence actually satisfiable from inside the cluster. See
# crates/api/src/data/legacy_backfill.rs's module doc.
#
# Migrations note: `crates/api/src/main.rs` runs `sqlx::migrate!().run(...)`
# with no path argument, which defaults to the `migrations/` directory next
# to this crate's `Cargo.toml` (`crates/api/migrations/`). `sqlx::migrate!`
# is a compile-time macro that embeds each migration file's contents (and
# checksums) into the binary via `include_str!`-style codegen — the
# `Migrator` it produces carries the SQL in memory, it does not re-read the
# `migrations/` directory at runtime. So the runtime image below does NOT
# copy `crates/api/migrations/` in; only the compiled binary is needed.
#
# Build from the repo root so the workspace's `Cargo.toml`/`Cargo.lock` and
# `crates/common` path dependency are all in the build context:
#   docker build -f docker/api.Dockerfile .
#
# CARGO_PROFILE picks the cargo build profile (and matching target/<profile>
# output dir): "release" (default) for optimized builds, "debug" for fast
# unoptimized dev builds. Set to "debug" by docker-compose.dev.yml, the
# override that `dev.env` selects via COMPOSE_FILE; docker-compose.yml on
# its own leaves it at "release".
ARG CARGO_PROFILE=release

FROM rust:1.88-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS builder
ARG CARGO_PROFILE

WORKDIR /app
COPY . .
# BuildKit cache mounts: the cargo registry, the git checkouts and the
# target/ dir all live in caches that persist across builds, so a rebuild
# recompiles only what actually changed instead of the whole dependency
# tree. Requires the `# syntax=` directive at the top of this file.
#
# The target cache id is keyed by rustc version (`cargo-target-1.88`). Every
# Rust service in this workspace now builds with the same rustc version, so
# this id is shared across all of them -- see docker-compose.yml's top-of-file
# comment for the full list. The registry and git caches hold only downloaded
# sources, so sharing those across all of them is safe too.
#
# `sharing=locked` because docker-compose builds services in parallel, and
# concurrent cargo invocations must not share one target dir unserialised.
#
# The trailing `cp` is the non-obvious part: a cache mount is NOT part of the
# resulting image layer, so /app/target ceases to exist the moment this RUN
# finishes and a later `COPY --from=builder /app/target/...` would find
# nothing. The binary has to be copied out to a normal path within the same
# RUN — which is why the runtime stage below copies from /usr/local/bin/api.
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-1.88,target=/app/target,sharing=locked \
    if [ "${CARGO_PROFILE}" = "release" ]; then \
      cargo build --release --bin api --bin backfill_trains --bin backfill_incident_lines --bin corpus_compare --bin replay_uidless_movements --bin backfill_line_train_summaries; \
    else \
      cargo build --bin api --bin backfill_trains --bin backfill_incident_lines --bin corpus_compare --bin replay_uidless_movements --bin backfill_line_train_summaries; \
    fi \
    && cp "/app/target/${CARGO_PROFILE}/api" /usr/local/bin/api \
    && cp "/app/target/${CARGO_PROFILE}/backfill_trains" /usr/local/bin/backfill_trains \
    && cp "/app/target/${CARGO_PROFILE}/backfill_incident_lines" /usr/local/bin/backfill_incident_lines \
    && cp "/app/target/${CARGO_PROFILE}/corpus_compare" /usr/local/bin/corpus_compare \
    && cp "/app/target/${CARGO_PROFILE}/replay_uidless_movements" /usr/local/bin/replay_uidless_movements \
    && cp "/app/target/${CARGO_PROFILE}/backfill_line_train_summaries" /usr/local/bin/backfill_line_train_summaries

FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251

# tini is PID 1, not the service binary. A process running as PID 1 gets no
# default signal handling from the kernel: SIGTERM without a handler of its
# own is ignored, so on every rollout or drain the container sat out the
# whole terminationGracePeriodSeconds and was SIGKILLed. tini forwards
# SIGTERM/SIGINT to the binary (which then exits, or shuts down gracefully
# where it installs a handler, e.g. notifier) and exits with its exit code,
# and it reaps orphaned children (api's own `parse-ticket` re-exec,
# `kubectl exec` helpers) instead of leaving zombies. No `-g`: only the
# binary is signalled, as in the pgBackRest image (docs/postgres-pitr.md,
# "Why tini"). A chart or `kubectl run --command` override of `command:`
# replaces this ENTRYPOINT outright, tini included; that is fine for the
# one-shot tools those overrides run.
#
# Debian bookworm's tini, pinned to its source version. The trailing `*` in
# the install matches Debian's binNMU suffix (`+b3` today): the archive keeps
# only the current rebuild, so an exact `+bN` pin would stop installing at
# the next rebuild (the DL3008 reason in .hadolint.yaml). apt verifies the
# .deb's SHA-256 against the base image's signed archive index, and the base
# image is digest-pinned. Upstream tini has been 0.19.0 since 2020; a new
# Debian source version only arrives with a new Debian release, and then
# the install fails loudly until this is bumped. Not tracked by Renovate:
# a deb datasource would propose exact `+bN` pins.
ARG TINI_VERSION=0.19.0-1

# sqlx's tls-native-tls feature verifies the Postgres connection's cert (when
# TLS is in play) against the system store, so the runtime image needs a CA
# bundle even though it otherwise only carries the one binary. `curl` is
# added on top of the poller Dockerfiles' pattern solely so docker-compose's
# HEALTHCHECK can probe `GET /public/health` from inside the container.
# hadolint ignore=DL3008 # apt versions unpinned on purpose; see .hadolint.yaml
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl "tini=${TINI_VERSION}*" \
    && rm -rf /var/lib/apt/lists/* \
    && tini --version \
    && groupadd --system --gid 1000 api \
    && useradd --system --no-create-home --shell /usr/sbin/nologin --uid 1000 --gid 1000 api

COPY --from=builder /usr/local/bin/api /usr/local/bin/api
# The operational, one-off backfill this image must be able to run BEFORE it
# is first started against a database with pre-existing `tracked_trains`
# data -- see crates/api/src/data/legacy_backfill.rs's module doc for the
# required deploy sequence. Shipped in the same image (rather than a second
# one) so it is guaranteed to be the exact build whose migrations are about
# to run:
#   kubectl run ... --image=<this image> --command -- /usr/local/bin/backfill_trains
# `api`'s own startup refuses to apply the contract migration until this has
# been run, so the two can never get out of order silently.
COPY --from=builder /usr/local/bin/backfill_trains /usr/local/bin/backfill_trains
# The one-off `incidents.affected_lines` backfill. Unlike `backfill_trains`
# nothing refuses to start without it -- the archive's Line filter simply
# finds no pre-existing rows until it has run, which is the behaviour it
# had before the column existed. It reads the catalogue from `/app/lines`
# (copied in just below), the same default as `api`'s own `--lines-dir`, so
# it needs no arguments here either:
#   kubectl run ... --image=<this image> --command -- /usr/local/bin/backfill_incident_lines
COPY --from=builder /usr/local/bin/backfill_incident_lines /usr/local/bin/backfill_incident_lines
# Read-only CORPUS-vs-timetable crosswalk report (api::data::corpus_comparison),
# run in the api pod with its own DATABASE_URL:
#   kubectl exec deploy/<api deployment> -c api -- corpus_compare [--full]
COPY --from=builder /usr/local/bin/corpus_compare /usr/local/bin/corpus_compare
# One-off, idempotent: writes the shared movement tables for the uid-less
# TRUST rows a trust-backlog-consumer restart left out of them (2026-10-01),
# from trust_event_backlog (kept a day), with the api pod's DATABASE_URL:
#   kubectl exec deploy/<api deployment> -c api -- replay_uidless_movements [<since, RFC 3339>]
COPY --from=builder /usr/local/bin/replay_uidless_movements /usr/local/bin/replay_uidless_movements
# Optional, idempotent: derives `line_train_summaries` for line populations
# stored before the table existed (or derived against an older catalogue);
# until then the line page reads the population JSONB. Reads /app/lines:
#   kubectl exec deploy/<api deployment> -c api -- backfill_line_train_summaries
COPY --from=builder /usr/local/bin/backfill_line_train_summaries /usr/local/bin/backfill_line_train_summaries
COPY --chown=api:api lines/ /app/lines/

# Numeric USER, not the `api` name useradd created above: Kubernetes'
# runAsNonRoot admission check (this chart's podSecurityContext sets
# runAsNonRoot: true with no explicit runAsUser) resolves the image's
# config purely from its manifest -- it does NOT read /etc/passwd inside
# the image -- so a symbolic USER fails admission with "container has
# runAsNonRoot and image has non-numeric user, cannot verify user is
# non-root". Pinned to the same uid/gid useradd was given above so this
# stays in sync with the group ownership set via COPY --chown/groupadd.
USER 1000:1000

# `api` also re-executes ITSELF (`std::env::current_exe()`, i.e. this path)
# as `api parse-ticket <pdf|pkpass>` for every ticket upload: the parse runs
# in that short-lived, rlimited child so it can be killed on timeout (M13;
# crates/api/src/data/ticket_subprocess.rs). No second binary, no temp
# files (stdin/stdout only), no extra capability -- so nothing here or in
# the chart's securityContext (readOnlyRootFilesystem, drop ALL,
# RuntimeDefault seccomp) needs to change for it. Keep the binary at this
# path and don't replace it in a running container.
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/api"]
