# syntax=docker/dockerfile:1
# Multi-stage build for the `frontend` service.
#
# Build from the repo root so relative paths work the same way as every
# other service's Dockerfile in this project:
#   docker build -f frontend/Dockerfile -t frontend .
#
# Two runtime targets, selected via docker-compose's `build.target`:
#   runtime-prod (default) - optimized `next build` + `next start`.
#                            Selected by docker-compose.yml.
#   runtime-dev             - skips `next build` entirely (ships full
#                              source + node_modules) and runs `next dev`.
#                            Selected by docker-compose.dev.yml, which
#                            `dev.env` pulls in via COMPOSE_FILE.
# builder-base is shared so the prod-only `npm run build` step never runs
# when only the dev target is requested.
FROM node:22-bookworm-slim AS builder-base

WORKDIR /app
COPY frontend/package.json frontend/package-lock.json ./
# BuildKit cache mount for npm's download cache, so a package-lock change
# re-resolves without re-fetching every tarball. This stage runs as root and
# node:22-bookworm-slim leaves npm's cache at its default $HOME/.npm, i.e.
# /root/.npm (verified with `npm config get cache` in the image) — mounting
# the wrong path would silently cache nothing. The installed node_modules
# itself is a normal layer, not a cache mount: runtime-dev's anonymous
# volume is seeded from it, so it must survive in the image.
RUN --mount=type=cache,id=npm,target=/root/.npm npm ci
COPY frontend/ .

FROM builder-base AS builder-prod
# API_BASE_URL is read at request time in Server Components (not baked
# into the build), so no build-time env var is needed here.
#
# Only .next/cache is a BuildKit cache mount — that is Next's incremental
# compiler cache, and it is the one part of .next/ that does not belong in
# the image. The rest of /app/.next must stay a real layer because
# runtime-prod copies it in below, and a cache mount is not part of the
# resulting layer.
RUN --mount=type=cache,id=next-cache,target=/app/.next/cache npm run build

FROM node:22-bookworm-slim AS runtime-prod

RUN useradd --system --no-create-home --shell /usr/sbin/nologin frontend

WORKDIR /app
COPY --from=builder-prod --chown=frontend:frontend /app/.next ./.next
COPY --from=builder-prod --chown=frontend:frontend /app/node_modules ./node_modules
COPY --from=builder-prod --chown=frontend:frontend /app/package.json ./package.json
# No `public/` directory: this plan adds no static assets (no favicon,
# no images). Next.js's `public/` dir is optional — omitting this COPY
# is correct, not a shortcut. If a later change adds static assets under
# `frontend/public/`, add `COPY --from=builder-prod --chown=frontend:frontend
# /app/public ./public` back in at that point.

USER frontend

EXPOSE 3000
ENTRYPOINT ["npm", "run", "start"]

FROM node:22-bookworm-slim AS runtime-dev

# No custom user here (unlike runtime-prod above): this stage deliberately
# runs as the `node` user that node:22-bookworm-slim already ships, which
# is uid/gid 1000 — the same as the developer's host account. That matters
# because docker-compose.dev.yml bind-mounts the host's ./frontend over
# /app: files the dev server writes into the mount (next-env.d.ts, tsconfig
# rewrites) then land host-owned rather than root-owned, and host-owned
# sources stay writable inside the container. Re-creating a `frontend` user
# is not an option anyway — `useradd --uid 1000` fails because the `node`
# user already holds 1000. For a host whose uid is NOT 1000, see the
# commented `user: "${DEV_UID}:${DEV_GID}"` escape hatch in
# docker-compose.dev.yml.

WORKDIR /app
# `next dev` writes its cache/HMR state under .next/ at runtime (unlike
# `next start`, which only reads the prebuilt output copied in below), so
# `node` needs write access to /app itself, not just the files COPY
# --chown places inside it - WORKDIR creates /app as root, and --chown only
# applies to the copied entries, not the pre-existing directory.
#
# .next/ is created here rather than left to the dev server because
# docker-compose.dev.yml mounts an ANONYMOUS volume at /app/.next. Docker
# seeds such a volume from whatever the image has at that path, inheriting
# its ownership; with no directory to seed from it creates an empty
# root-owned one instead and `next dev` cannot write its cache.
RUN mkdir -p /app/.next && chown -R node:node /app
# The bind mount shadows this copy at runtime, so it is not what the dev
# server reads. It is still load-bearing: it is what populates the
# anonymous /app/node_modules volume with container-native (node 22 /
# Debian) modules, which is why the root .dockerignore must keep the host's
# own frontend/node_modules out of the build context.
COPY --from=builder-base --chown=node:node /app ./

USER node

EXPOSE 3000
ENTRYPOINT ["npm", "run", "dev"]
