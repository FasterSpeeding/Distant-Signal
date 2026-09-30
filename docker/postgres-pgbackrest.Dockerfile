# syntax=docker/dockerfile:1@sha256:ecfaec9ed6d810b56388c508f4121597bfbba70d41a6dfeee4d8cad5f295fc32
# The bundled Postgres plus pgBackRest, for point-in-time recovery
# (charts/distant-signal `postgresql.pgbackrest`; design:
# docs/superpowers/specs/2026-09-30-backup-and-observability-gaps-design.md,
# item 1; runbook: docs/postgres-pitr.md).
#
# The base MUST stay the exact image `postgresql.image.tag` pins in
# charts/distant-signal/values.yaml (same version, same `-trixie` Debian
# base, same digest): the chart swaps this image in for that one, over the
# same data directory. A different Debian base changes glibc, whose
# collation changes can silently corrupt text indexes. Renovate groups the
# two bumps (renovate.json5, "postgres 16").
#
# pgBackRest comes from the PGDG apt repository the base image already
# configures (trixie-pgdg). The version is pinned, unlike this repo's
# Debian-archive installs (see .hadolint.yaml, DL3008): apt.postgresql.org
# keeps every published version in its index, so a pin keeps building, and
# the repository format is only guaranteed within one pgBackRest version
# line. Renovate bumps it through the deb datasource (renovate.json5).
#
# No USER: the image's entrypoint drops to `postgres` via gosu, and the
# chart already runs the pod as uid/gid 999, exactly as for the stock image.
#
# Build from the repo root:
#   docker build -f docker/postgres-pgbackrest.Dockerfile .
FROM postgres:16.15-trixie@sha256:1a6ab3f5345eb6dbe04a1349529caabdb0ab09293a09590fad07b2246bfa4b54

ARG PGBACKREST_VERSION=2.59.1-1.pgdg13+1

# ca-certificates: pgBackRest verifies the S3 endpoint's certificate
# against the system bundle, and the base image purges the package after
# fetching gosu. It comes from the Debian archive, so it stays unpinned like
# every other Debian install in this repo.
# hadolint ignore=DL3008 # ca-certificates unpinned on purpose; see .hadolint.yaml
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates "pgbackrest=${PGBACKREST_VERSION}" \
    && rm -rf /var/lib/apt/lists/* \
    && pgbackrest version

# The daily check the chart's `<release>-pgbackrest-check` CronJob runs
# inside this container: `pgbackrest check`, `pgbackrest verify`, and the
# WAL gap check that catches segments archive-push-queue-max dropped.
COPY --chmod=0755 docker/pgbackrest/pgbackrest-daily-check.sh /usr/local/bin/pgbackrest-daily-check
