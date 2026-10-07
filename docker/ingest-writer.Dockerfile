# syntax=docker/dockerfile:1@sha256:ecfaec9ed6d810b56388c508f4121597bfbba70d41a6dfeee4d8cad5f295fc32
# Multi-stage build for the `ingest-writer` service (ingest architecture,
# plan 1B.6). Same shape as docker/aggregator.Dockerfile: rust:1.88 builder
# (the workspace floor), the shared BuildKit caches, and a slim runtime with
# the CA bundle, the line catalogue and a numeric non-root USER. See that
# file for the reasoning behind each step.
#
# Kept deliberately plain: the image Dockerfiles are being reworked for
# cargo-chef in parallel, and this one should follow whatever shape they
# take then.
#
# Build from the repo root:
#   docker build -f docker/ingest-writer.Dockerfile .
ARG CARGO_PROFILE=release

FROM rust:1.88-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS builder
ARG CARGO_PROFILE

WORKDIR /app
COPY . .
# The cache mount is not part of the image layer, so the binary is copied
# out of /app/target within the same RUN.
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-1.88,target=/app/target,sharing=locked \
    if [ "${CARGO_PROFILE}" = "release" ]; then \
      cargo build --release --bin ingest-writer; \
    else \
      cargo build --bin ingest-writer; \
    fi \
    && cp "/app/target/${CARGO_PROFILE}/ingest-writer" /usr/local/bin/ingest-writer

FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251

# tini as PID 1: it forwards SIGTERM to the binary (which then closes its
# advisory-lock session, so a standby takes the loops over at once) and
# reaps zombies. Installed the same way as in every other service image:
# Debian bookworm's tini, pinned to its source version, with `*` for the
# binNMU suffix.
ARG TINI_VERSION=0.19.0-1

# sqlx's tls-native-tls verifies a TLS Postgres connection against the
# system store, hence the CA bundle.
# hadolint ignore=DL3008 # apt versions unpinned on purpose; see .hadolint.yaml
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates "tini=${TINI_VERSION}*" \
    && rm -rf /var/lib/apt/lists/* \
    && tini --version \
    && groupadd --system --gid 1000 ingest-writer \
    && useradd --system --no-create-home --shell /usr/sbin/nologin --uid 1000 --gid 1000 ingest-writer

COPY --from=builder /usr/local/bin/ingest-writer /usr/local/bin/ingest-writer
# The line catalogue (`LINES_DIR`, default /app/lines): the sweeps build
# their CRS-to-line index from it.
COPY --chown=ingest-writer:ingest-writer lines/ /app/lines/

# Numeric, for the chart's runAsNonRoot admission check.
USER 1000:1000

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/ingest-writer"]
