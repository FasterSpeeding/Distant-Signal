# syntax=docker/dockerfile:1
# Multi-stage build for the `schedule-reference` service. See
# docker/schedule-ingest.Dockerfile's own header comment for the full
# rationale behind the rust:1.88-bookworm pin and the cache-mount shape --
# unchanged here, this crate shares the same `common`-crate-driven
# reqwest -> url -> idna -> icu_* transitive dependency chain.
#
# Build from the repo root:
#   docker build -f docker/schedule-reference.Dockerfile .
ARG CARGO_PROFILE=release

FROM rust:1.88-bookworm AS builder
ARG CARGO_PROFILE

WORKDIR /app
COPY . .
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-1.88,target=/app/target,sharing=locked \
    if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo build --release --bin schedule-reference; \
    else \
      cargo build --bin schedule-reference; \
    fi \
    && cp /app/target/${CARGO_PROFILE}/schedule-reference /usr/local/bin/schedule-reference

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin poller

COPY --from=builder /usr/local/bin/schedule-reference /usr/local/bin/schedule-reference

USER poller

ENTRYPOINT ["/usr/local/bin/schedule-reference"]
