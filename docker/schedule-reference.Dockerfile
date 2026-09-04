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
    && groupadd --system --gid 1000 poller \
    && useradd --system --no-create-home --shell /usr/sbin/nologin --uid 1000 --gid 1000 poller

COPY --from=builder /usr/local/bin/schedule-reference /usr/local/bin/schedule-reference
# Task 7's second responsibility (per-line CIF SCHEDULE population publish)
# reads the same static line catalogue `aggregator`/`api` already bake in
# the same way (see those Dockerfiles' own identical COPY step) -- this
# crate had no use for `lines/` before Task 7.
COPY --chown=poller:poller lines/ /app/lines/

# Numeric USER, not the `poller` name useradd created above: Kubernetes'
# runAsNonRoot admission check (this chart's podSecurityContext sets
# runAsNonRoot: true with no explicit runAsUser) resolves the image's
# config purely from its manifest -- it does NOT read /etc/passwd inside
# the image -- so a symbolic USER fails admission with "container has
# runAsNonRoot and image has non-numeric user, cannot verify user is
# non-root". Pinned to the same uid/gid useradd was given above so this
# stays in sync with the group ownership set via COPY --chown/groupadd.
USER 1000:1000

ENTRYPOINT ["/usr/local/bin/schedule-reference"]
