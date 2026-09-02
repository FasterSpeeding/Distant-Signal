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

- `crates/` — a ten-crate Rust workspace: `common`, `api`, `aggregator`,
  `enricher`, `trust-consumer`, and five `poller-*` crates.
- `frontend/` — the Next.js web frontend.
- `charts/distant-signal/` — the Helm chart for deploying the whole stack.
- `vault/` — `install.sh` + `values.yaml` for standing up OpenBao (an
  open-source, MPL-2.0 HashiCorp Vault fork/alternative — see
  `vault/install.sh`'s header for why) in a Kubernetes cluster via its
  official Helm chart, wired up to the `devAuthentik` dev IdP
  (`charts/distant-signal/values.yaml`'s `devAuthentik.*`) for OIDC login.
- `lines/` — the curated TOML line-definition catalogue (unchanged from
  this project's original design).

See `DESIGN.md` for the full architecture.

## Running it

For local development, see `docker-compose.yml`. For a real deployment, see
`charts/distant-signal/README.md` for the Helm chart.

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
  suppressed entirely if a more precise match exists for the same incident.

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
values (Recovering = 20, Diverted = 21) outside the TfL range to avoid
clashes if TfL adds new codes.

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
