# Multi-stage build for the `frontend` service.
#
# Build from the repo root so relative paths work the same way as every
# other service's Dockerfile in this project:
#   docker build -f frontend/Dockerfile -t frontend .
#
# Two runtime targets, selected via docker-compose's `build.target`
# (see docker-compose.yml, `--profile dev`):
#   runtime-prod (default) - optimized `next build` + `next start`.
#   runtime-dev             - skips `next build` entirely (ships full
#                              source + node_modules) and runs `next dev`.
# builder-base is shared so the prod-only `npm run build` step never runs
# when only the dev target is requested.
FROM node:22-bookworm-slim AS builder-base

WORKDIR /app
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ .

FROM builder-base AS builder-prod
# API_BASE_URL is read at request time in Server Components (not baked
# into the build), so no build-time env var is needed here.
RUN npm run build

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

RUN useradd --system --no-create-home --shell /usr/sbin/nologin frontend

WORKDIR /app
# `next dev` writes its cache/HMR state under .next/ at runtime (unlike
# `next start`, which only reads the prebuilt output copied in below), so
# frontend needs write access to /app itself, not just the files COPY
# --chown places inside it - WORKDIR creates /app as root, and --chown only
# applies to the copied entries, not the pre-existing directory.
RUN chown frontend:frontend /app
COPY --from=builder-base --chown=frontend:frontend /app ./

USER frontend

EXPOSE 3000
ENTRYPOINT ["npm", "run", "dev"]
