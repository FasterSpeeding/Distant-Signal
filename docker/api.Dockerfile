# syntax=docker/dockerfile:1
# Multi-stage build for the `api` service.
#
# Builder pin: edition 2024 (used by every crate in this workspace) needs
# rustc 1.85+, and the three poller Dockerfiles pin 1.86 because their
# resolved Cargo.lock pulls in transitive icu_* deps (via reqwest) needing
# 1.86+. `api` doesn't depend on reqwest, but it pulls in `sqlx-postgres`,
# whose transitive `home` crate (pinned to 0.5.12 in the workspace
# Cargo.lock) requires rustc 1.88+ — confirmed by actually building this
# image against rust:1.86-bookworm first and hitting:
#   "error: rustc 1.86.0 is not supported ... home@0.5.12 requires rustc 1.88"
# 1.88 is the real floor for *this* crate's dependency tree, one minor
# version above the other three services.
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

FROM rust:1.88-bookworm AS builder
ARG CARGO_PROFILE

WORKDIR /app
COPY . .
# BuildKit cache mounts: the cargo registry, the git checkouts and the
# target/ dir all live in caches that persist across builds, so a rebuild
# recompiles only what actually changed instead of the whole dependency
# tree. Requires the `# syntax=` directive at the top of this file.
#
# The target cache id is keyed by rustc version (`cargo-target-1.88`) rather
# than shared across all six Rust services. Cargo's fingerprints include the
# compiler version, so the 1.88 services (api, aggregator) and the 1.86 ones
# (the four pollers) would otherwise invalidate and fully recompile each
# other's artifacts on every alternating build. The registry and git caches
# hold only downloaded sources, so sharing those across all six is safe.
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
    if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo build --release --bin api; \
    else \
      cargo build --bin api; \
    fi \
    && cp /app/target/${CARGO_PROFILE}/api /usr/local/bin/api

FROM debian:bookworm-slim

# sqlx's tls-native-tls feature verifies the Postgres connection's cert (when
# TLS is in play) against the system store, so the runtime image needs a CA
# bundle even though it otherwise only carries the one binary. `curl` is
# added on top of the poller Dockerfiles' pattern solely so docker-compose's
# HEALTHCHECK can probe `GET /public/health` from inside the container.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin api

COPY --from=builder /usr/local/bin/api /usr/local/bin/api
COPY --chown=api:api lines/ /app/lines/

USER api

ENTRYPOINT ["/usr/local/bin/api"]
