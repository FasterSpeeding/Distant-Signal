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
FROM rust:1.88-bookworm AS builder

WORKDIR /app
COPY . .
RUN cargo build --release --bin api

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

COPY --from=builder /app/target/release/api /usr/local/bin/api

USER api

ENTRYPOINT ["/usr/local/bin/api"]
