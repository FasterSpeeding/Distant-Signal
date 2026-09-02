# syntax=docker/dockerfile:1
#
# Multi-stage build for `orchestrator/`, the DS-hosted chat orchestrator
# (embedded-chatbot-option-b plan, Task 3/6). Built from the repo root, same
# as every other service's Dockerfile in this project:
#   docker build -f orchestrator/Dockerfile -t chat-orchestrator .
#
# Named stages (builder -> runtime-prod) deliberately mirror
# frontend/Dockerfile's own naming convention (the one other TypeScript
# service this repo already builds this way) -- see the embedded-chatbot-
# option-b plan's own Global Constraints for why orchestrator/ follows that
# precedent rather than distant-signal-mcp's own Dockerfile shape (a
# different repository's own convention, not this one's). Unlike
# frontend/Dockerfile, there is no separate `runtime-dev` stage: this
# service has no dev-server/HMR story to preserve, so `runtime-prod` is
# this Dockerfile's only, final stage -- `.github/workflows/containers.yml`'s
# matrix entry for this image needs no explicit `target`, same as the nine
# Rust services' entries.
FROM node:22-bookworm-slim AS builder

WORKDIR /app
COPY orchestrator/package.json orchestrator/package-lock.json ./
# BuildKit cache mount for npm's download cache -- same rationale as
# frontend/Dockerfile's identical builder-base step.
RUN --mount=type=cache,id=npm-orchestrator,target=/root/.npm npm ci
COPY orchestrator/ .
RUN npm run build

FROM node:22-bookworm-slim AS runtime-prod

RUN useradd --system --no-create-home --shell /usr/sbin/nologin orchestrator

WORKDIR /app
COPY orchestrator/package.json orchestrator/package-lock.json ./
# Production dependencies only -- the builder stage's node_modules includes
# devDependencies (typescript, vitest, tsx) this runtime image has no use
# for. Same cache-mount id convention as the install above (a distinct id
# from it since --omit=dev resolves a different dependency set).
RUN --mount=type=cache,id=npm-orchestrator-prod,target=/root/.npm npm ci --omit=dev
COPY --from=builder --chown=orchestrator:orchestrator /app/dist ./dist

USER orchestrator

EXPOSE 3001
ENTRYPOINT ["node", "dist/src/index.js"]
