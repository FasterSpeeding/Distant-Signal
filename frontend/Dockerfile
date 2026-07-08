# Multi-stage build for the `frontend` service.
#
# Build from the repo root so relative paths work the same way as every
# other service's Dockerfile in this project:
#   docker build -f frontend/Dockerfile -t frontend .
FROM node:22-bookworm-slim AS builder

WORKDIR /app
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ .
# API_BASE_URL is read at request time in Server Components (not baked
# into the build), so no build-time env var is needed here.
RUN npm run build

FROM node:22-bookworm-slim

RUN useradd --system --no-create-home --shell /usr/sbin/nologin frontend

WORKDIR /app
COPY --from=builder --chown=frontend:frontend /app/.next ./.next
COPY --from=builder --chown=frontend:frontend /app/node_modules ./node_modules
COPY --from=builder --chown=frontend:frontend /app/package.json ./package.json
# No `public/` directory: this plan adds no static assets (no favicon,
# no images). Next.js's `public/` dir is optional — omitting this COPY
# is correct, not a shortcut. If a later change adds static assets under
# `frontend/public/`, add `COPY --from=builder --chown=frontend:frontend
# /app/public ./public` back in at that point.

USER frontend

EXPOSE 3000
ENTRYPOINT ["npm", "run", "start"]
