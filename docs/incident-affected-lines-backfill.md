# Backfilling `incidents.affected_lines`

`crates/api/migrations/20260917090000_incidents_affected_lines.sql` adds
`incidents.affected_lines`, the column the incident archive's Line filter
(`GET /public/incidents?line=...`) now matches against. New and still-live
incidents fill it automatically; **rows already in the table do not**, and
they are the ones the archive exists to serve.

## Why a backfill is needed at all

`queries::upsert_incidents` computes `affected_lines` for every incident it
writes, and `poller-incidents` re-sends the entire current feed every cycle
— so any incident still in the feed is populated within one poll of the
deploy. An incident that has already dropped out of the feed is never
written again, so it keeps the migration's `'{}'` default forever and the
Line filter will not find it.

At the time this was written the production archive held **1507 incidents,
every one of them with an empty `affected_stations`** (the column the Line
filter used to match on, which no production writer has ever populated —
RDM's Knowledgebase Incidents XML has no CRS field, only a free-text
`RoutesAffected`). That is why the filter returned zero rows for every
line, National Rail included. See
`docs/superpowers/specs/2026-09-16-tfl-incident-archive-design.md` §1c.

## Running it

Idempotent, re-runnable, and safe at any time — including while the poller
and `api` are running. It writes only `affected_lines`, only for rows whose
recomputed value differs from what is stored, so a second run reports zero
updates.

```sh
# From a checkout
DATABASE_URL=postgres://... LINES_DIR=./lines \
  cargo run -p api --bin backfill_incident_lines

# From the api container image (the binary ships alongside `api` itself,
# and defaults LINES_DIR to the image's own /app/lines)
/usr/local/bin/backfill_incident_lines
```

In the production cluster, the usual shape is a one-off pod from the same
image the running `api` was built from:

```sh
kubectl -n distant-signal run backfill-incident-lines \
  --rm -it --restart=Never \
  --image=<the image the api Deployment is running> \
  --env=DATABASE_URL=<the api Deployment's DATABASE_URL> \
  --command -- /usr/local/bin/backfill_incident_lines
```

## Reading the output

```
backfill complete:
  incidents examined:            1507
  incidents updated:             1400
  incidents matching no line:     107
```

- **examined** — every row in `incidents`.
- **updated** — rows whose `affected_lines` changed. On a first run this
  is "rows that now match at least one line"; on a re-run after a
  `lines/*.toml` edit it is "rows the catalogue change moved".
- **matching no line** — rows that genuinely match no catalogue line. This
  is expected, not an error: the Knowledgebase feed carries incidents for
  operators and routes with no `lines/*.toml` entry. Those rows stay
  reachable through the archive's Operator filter, exactly as before.

A run that reports `updated: 0` on a database that has never been
backfilled almost certainly means the catalogue was not found. The binary
refuses to start on an empty catalogue for that reason (an empty catalogue
would otherwise *clear* every row), so check `LINES_DIR`.

## When to re-run

Any time `lines/*.toml` changes in a way that affects matching —
`operators`, `match_keywords`, `excluded_keywords`, or a station's
`segment`. Live incidents pick the change up on the next poll on their own;
archived ones only move when this runs.

Implementation and reasoning: `crates/api/src/data/incident_line_backfill.rs`.
