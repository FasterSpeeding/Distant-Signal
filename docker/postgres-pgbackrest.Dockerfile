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
# tini is PID 1, not the postmaster (see the ENTRYPOINT below). Every
# published tag names the tini version too
# (`pg<postgres>-pgbackrest<version>-tini<version>`, containers.yml).
#
# Build from the repo root:
#   docker build -f docker/postgres-pgbackrest.Dockerfile .
FROM postgres:16.15-trixie@sha256:1a6ab3f5345eb6dbe04a1349529caabdb0ab09293a09590fad07b2246bfa4b54

ARG PGBACKREST_VERSION=2.59.1-1.pgdg13+1
# Debian trixie's tini, pinned to its source version. The trailing `*` in
# the install below matches Debian's binNMU suffix (`+b8` today): tini is
# rebuilt against each glibc point release (Built-Using: glibc), and the
# archive keeps only the current rebuild, so an exact `+bN` pin would stop
# building at the next glibc update (the DL3008 reason in .hadolint.yaml).
# Upstream tini has been 0.19.0 since 2020; a new Debian source version
# only arrives with the base image's Debian release, and then this fails
# the build loudly until it is bumped. Not tracked by Renovate: a deb
# datasource would propose exact `+bN` pins.
ARG TINI_VERSION=0.19.0-3

# ca-certificates: pgBackRest verifies the S3 endpoint's certificate
# against the system bundle, and the base image purges the package after
# fetching gosu. It comes from the Debian archive, so it stays unpinned like
# every other Debian install in this repo.
# hadolint ignore=DL3008 # ca-certificates unpinned on purpose; see .hadolint.yaml
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
        "pgbackrest=${PGBACKREST_VERSION}" "tini=${TINI_VERSION}*" \
    && rm -rf /var/lib/apt/lists/* \
    && pgbackrest version \
    && tini --version

# The daily check the chart's `<release>-pgbackrest-check` CronJob runs
# inside this container: `pgbackrest check`, `pgbackrest verify`, and the
# WAL gap check that catches segments archive-push-queue-max dropped.
COPY --chmod=0755 docker/pgbackrest/pgbackrest-daily-check.sh /usr/local/bin/pgbackrest-daily-check

# tini as PID 1, so the postmaster is not. pgBackRest's async archive-push
# (the chart's archive_command, archive-async=y) forks a detached worker
# that the kernel reparents to PID 1. With the postmaster as PID 1, the
# worker's exit reached the postmaster as a child it never started, which
# it handles like a crashed backend: on a failing push (exit 103, e.g. no
# stanza yet, or S3 unreachable) it logged "server process (PID n) exited
# with exit code 103", killed every connection and ran crash recovery,
# again with every failed push (seen in production). tini reaps
# such orphans instead, and a failing push just retries.
#
# The rest reproduces the base image: its ENTRYPOINT, CMD (a new ENTRYPOINT
# resets the inherited CMD) and STOPSIGNAL (SIGINT: Postgres' fast
# shutdown; containerd sends the image's stop signal). tini forwards
# SIGINT/SIGTERM to the postmaster only, without `-g`: the postmaster shuts
# its own children down in order (they are in their own sessions anyway),
# and signalling them directly would mean a different shutdown mode. tini
# exits with the postmaster's exit code.
#
# The chart sets only `args` (the `-c` settings) for this container, never
# `command`, so this ENTRYPOINT applies (a CI helm-lint step checks it).
ENTRYPOINT ["tini", "--", "docker-entrypoint.sh"]
CMD ["postgres"]
STOPSIGNAL SIGINT
