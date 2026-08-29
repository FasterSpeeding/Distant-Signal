# syntax=docker/dockerfile:1
# Multi-stage build for trust-consumer. Needs cmake + OpenSSL headers in
# the builder stage for rdkafka's `cmake-build`/`ssl` features (librdkafka
# is a C library rdkafka vendors and compiles from source) -- every other
# Rust service in this repo is a pure-Rust dependency tree and doesn't need
# this.
ARG CARGO_PROFILE=release

FROM rust:1.88-bookworm AS builder
ARG CARGO_PROFILE

RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-1.88,target=/app/target,sharing=locked \
    if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo build --release --bin trust-consumer; \
    else \
      cargo build --bin trust-consumer; \
    fi \
    && cp /app/target/${CARGO_PROFILE}/trust-consumer /usr/local/bin/trust-consumer

FROM debian:bookworm-slim

# `curl` is added on top of the poller Dockerfiles' pattern solely so
# docker-compose's HEALTHCHECK can probe `GET /healthz` from inside the
# container -- same reasoning as docker/api.Dockerfile's runtime stage.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin trust-consumer

COPY --from=builder /usr/local/bin/trust-consumer /usr/local/bin/trust-consumer

USER trust-consumer

ENTRYPOINT ["/usr/local/bin/trust-consumer"]
