# Distant Signal

A personal UK rail companion: line-status aggregation in the TfL-Unified-API
style, individual train tracking, accounts, and ticket/Delay-Repay support.
It has first-class support for operators with multiple parallel
routes that share trunk track (SWR, Southeastern, Northern, etc.) — knowing
the difference between an incident on a *shared trunk* (which should
propagate to every line using that trunk) and an incident on an *exclusive
segment* (which should not) is this project's original core and still its
real differentiator.

## Layout

- `crates/` — a 25-crate Rust workspace (see the root `Cargo.toml`):
  - services: `api`, `aggregator`, `enricher`, `notifier`;
  - eight `poller-*` crates: `poller-incidents`, `poller-stations`,
    `poller-tocs`, `poller-ldbws`, `poller-tfl`, `poller-irish-rail-gtfs`,
    `poller-irish-rail-live`, `poller-nir-stations`;
  - the TRUST train-movements pipeline: `movement-relay`, `trust-consumer`,
    `full-coverage-consumer`, `trust-backlog-consumer`;
  - the CIF schedule feed: `schedule-ingest`, `schedule-reference`;
  - libraries: `common`, `trust-schema`, `movement-feed`, `health-http`,
    `schedule-query`, `trip-planner`;
  - the `line-catalogue-validator` CLI.
- `frontend/` — the Next.js web frontend.
- `charts/distant-signal/` — the Helm chart for deploying the whole stack.
- `lines/` — the curated TOML line-definition catalogue, one file per line
  (format in `lines/SCHEMA.md`).
- `scripts/` — CI and maintenance tooling (Python, managed with uv; see
  below).

See `DESIGN.md` for the full architecture.

## Running it

For local development, see `docker-compose.yml` (its header explains the
`local.env` / `dev.env` modes). For a real deployment, see
`charts/distant-signal/README.md` for the Helm chart.

By default the local producers still POST to the api's `/private/*` routes.
To run the ingest paths production moves to (an `ingest-writer`, and every
producer writing Postgres or a Redis stream directly), append
`:docker-compose.direct.yml` to `COMPOSE_FILE` in your `local.env` or
`dev.env`; see that file's header and the comment beside `COMPOSE_FILE` in
either `*.env.example`. That overlay becomes the default when ingest phase 5
removes the HTTP paths (`docs/ingest-phase5-runbook.md`, step 5.3b).

## Scripts and their lint

The Python tooling is managed with [uv](https://docs.astral.sh/uv/):
`pyproject.toml` pins the lint tools in its `lint` dependency group,
`uv.lock` pins the full tree, and `.python-version` the interpreter. With uv
installed (e.g. `mise use -g uv`):

```sh
uv run scripts/lint-scripts.py         # what CI's scripts-lint job runs
uv run scripts/lint-scripts.py --fix   # apply shfmt/ruff fixes first
uv run python -m unittest discover -s scripts/tests   # the scripts' tests
# Before merging a branch that adds migrations (CI's migration-order job):
uv run scripts/check-migration-order.py "$(git merge-base main HEAD)"
```

A migration with destructive DDL (a `DROP` of a table, column, view or
function, a `RENAME`, a column type change, `SET NOT NULL` on an existing
column) needs a `-- contract: <what> (code stopped using it in <commit>)`
line in its leading comments, and ships a release after the code stopped
using the object; `check-migration-order.py` enforces the header (see its
docstring).

`uv run` creates `.venv/` and installs the `lint` group on first use. It runs
shellcheck and shfmt over every `*.sh`, ruff and mypy over `scripts/`,
actionlint over the workflows, hadolint over the Dockerfiles, and
`scripts/lint-containers.py`.

## How segments work

Each station on a line belongs to a named `segment`. When the same segment
name appears across multiple line definitions, the system treats it as a
shared trunk — incidents there propagate to every line using that segment.

The matcher classifies every incident-to-line match by scope:

- `EXCLUSIVE_SEGMENT` — incident's stations all sit on segments unique to
  this line. Highest confidence.
- `SHARED_SEGMENT` — at least one of the touched segments is shared.
  Status propagates to all lines using that segment, with a "shared trunk"
  annotation in the reason text.
- `STATION_HIT` — line/station overlap but no segment metadata to classify.
- `KEYWORD_ONLY` — line is named in the incident text but no station hits.
  Capped at Severe Delays.
- `OPERATOR_ONLY` — only operator overlap. Capped at Minor Delays, and
  suppressed entirely if another line sharing one of its operator codes got
  a more precise match for the same incident.

Knowledgebase incidents carry no station codes, so the station tiers are fed
by the stations an incident's text names, resolved against the station
reference data. An incident naming a place is shown only on the lines through
it; it is shown operator-wide only when its text is network-wide (industrial
action, a reduced timetable) or names no place at all.

The last point matters: it's what stops an incident on the Alton branch
from also flagging South West Main and Portsmouth Direct just because all
three share the `SW` operator code.

## Adding a complex operator

For a TOC like SWR with multiple routes:

1. Create one line file per passenger route (`swr-south-west-main.toml`,
   `swr-portsmouth-direct.toml`, etc.).
2. Use the same segment name (e.g. `swr-trunk-waterloo`) on all the lines
   that share trunk track. Junction stations belong to the shared trunk;
   exclusive segments start at the next station.
3. Set `destination_crs_filter` (and/or `headcode_prefixes`) so LDBWS
   inference at shared stations counts only the line's own services.
4. Add `match_keywords` for any colloquial line names ("Portsmouth Direct",
   "Alton line").
5. Run the test suite. Add a scenario that exercises the new line's shared
   trunks and exclusive segments — both shapes of incident must produce
   the right behaviour.

## Severity scale

We use TfL's 0-14 scale verbatim where it applies, then add two NR-specific
values (Recovering = 20, Diverted = 21) outside that range. TfL's own
codes 16-20 (Not Running, Issues Reported, No Issues, Information, Service
Closed) arrived later and are stored as 22-26, because 20 and 21 were
already taken (see `common::Severity`). The numbers are not ordered by how
bad a status is; compare through `common::severity_rank`.

## Design notes

- **Per-line thresholds matter.** A 5-minute delay on a 15-min-frequency
  commuter route is more disruptive than the same delay on an hourly
  long-distance route.
- **Knowledgebase prose is the gold.** When an active KB incident exists,
  prefer its description text as the `reason` over anything we infer.
- **Inference is a fallback, not a primary signal.** Only emit non-Good
  inferred statuses with reasonable sample sizes (`min_sample_size`).
- **Make data quality visible.** Clients should be able to tell whether a
  status came from a curated source or was inferred. We expose this via
  `dataQuality` on every status.
- **Junction stations belong to the shared trunk.** This is the single
  most important rule when authoring line definitions. The exclusive
  segment starts *after* the junction.
