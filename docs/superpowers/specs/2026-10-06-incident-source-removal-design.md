# Incident source removal ("Ended (no longer listed)") design

Status: implemented 2026-10-06. Supersedes the "implicitly cleared after N
polls" non-goal of
[2026-07-16-stale-incident-handling-design.md](2026-07-16-stale-incident-handling-design.md)
(see that spec's 2026-10-06 decision) and amends the archive's `cleared`
filter in
[2026-09-12-incident-archive-design.md](2026-09-12-incident-archive-design.md).

## Problem (investigated on production, 2026-10-06)

"Active" was computed only as `NOT incidents.is_cleared`, and `is_cleared` is
copied verbatim from RDM Knowledgebase's `ClearedIncident`
(`crates/poller-incidents/src/schema.rs`).

- The KB feed purges incidents nightly, at about 22:57-23:00 UTC, whatever
  their state. A row that drops out without `ClearedIncident=true` stayed
  active forever: 9 unplanned rows on 2026-10-06.
- RDM never clears planned rows, so every planned incident that ever left
  the feed was still "active": 398 rows.
- `upsert_incidents` only upserts. `fetched_at` is bumped only for the ids
  in the batch, so it already acts as a "last listed" timestamp, but absent
  rows were never touched.

The 2026-07-16 spec made "hasn't been refreshed in N poll cycles, so
implicitly cleared" a non-goal because incidents vanishing from the feed was
an unconfirmed failure mode. Production data now confirms it.

## Incident lifecycle

An incident is in exactly one of three states:

| State | Condition | Shown as |
| --- | --- | --- |
| Active | `NOT is_cleared AND source_removed_at IS NULL` | green "Active" |
| Cleared | `is_cleared` (RDM set `ClearedIncident`) | filled gray "Cleared" |
| Ended | `NOT is_cleared AND source_removed_at IS NOT NULL` | light gray "Ended", "No longer listed by the source since <time>" |

User decision 1: leaving the feed without RDM clearing it is a distinct
state, not "Cleared". `is_cleared` stays RDM's own fact. "Ended" is our
observation, so the UI says what we know ("no longer listed by the source")
rather than claiming the disruption is over.

The states are disjoint by construction: a listed row has its
`source_removed_at` reset to NULL, and only uncleared rows are ever marked.
Transitions:

- Active to Ended: missing from 2 consecutive complete polls (below).
- Ended to Active: listed again, in any snapshot, complete or not.
- Active or Ended to Cleared: RDM clears it while still listing it (the
  listing also un-ends it). A cleared row is never marked ended.

## Schema (`20261006120000_incidents_source_removed.sql`)

- `incidents.source_removed_at TIMESTAMPTZ NULL`: set to the row's
  `fetched_at` (when the feed last listed it) when it is marked ended.
- `incidents.source_missing_polls SMALLINT NOT NULL DEFAULT 0`: consecutive
  complete snapshots it was missing from.
- `incident_feed_state` (one row): `last_complete_at`,
  `last_complete_size`, the previous complete snapshot, for the shrink guard
  and the backfill.

All catalog-only (a nullable column, a constant-default NOT NULL column, a
new table). No index: the inference UPDATE and the `state` filter run
alongside predicates the existing indexes serve, over a few thousand rows.

**Why a counter rather than comparing `fetched_at` with snapshot times.**
"Missing from the last two complete snapshots" could be derived from
`fetched_at` and a log of complete-snapshot times. That needs the log, and
an incomplete snapshot in between would need careful handling. A counter
advanced only by guarded snapshots and reset by any listing is the simplest
correct form of user decision 2, and it is visible per row.

**Why `source_removed_at = fetched_at`, not `now()`.** `fetched_at` is the
best bound we have on when the incident left (some time after it), it does
not depend on when the second miss happened to be confirmed, and it is what
the one-off backfill writes too, so old and new removals read the same.

## Poller: snapshot completeness

`POST /private/incidents` takes a `common::IncidentSnapshot`:

```json
{"incidents": [IncidentMessage, ...], "complete": true, "skipped": 0}
```

`complete` is true only when no `<PtIncident>` was skipped as malformed and
the document's root element closed before EOF. The second condition matters:
a body cut off between two complete elements otherwise parses cleanly as a
shorter feed. Skips are counted as
`poller_incidents_skipped_elements_total`.

Compatibility: the api still accepts the old bare array, read as an
incomplete snapshot, and a snapshot without `complete` is incomplete too, so
an older poller can never trigger inference. An older api rejects the new
object with a 4xx, which fails just that poll cycle. Deploy the api first.

## API: the guard and the inference

`queries::upsert_incident_snapshot` upserts the batch in chunks, exactly as
before. Every listed incident gets `source_missing_polls = 0` and
`source_removed_at = NULL`. Then, once every chunk has committed,
`data::incident_removal::infer_removals` runs in one transaction. It reads
the baseline row `FOR UPDATE`, so overlapping POSTs run their inference one
after the other. It applies the inference only when ALL of these hold (user
decision 2):

1. the snapshot is `complete`;
2. it lists at least one incident;
3. the previous complete snapshot is at least 120 s old, so a retried POST
   whose first attempt already committed (a lost response) does not count
   the same poll twice. The poller polls every 300 s and stops retrying
   after a quarter of that, so a retry lands inside 120 s and the next real
   poll does not;
4. a previous complete snapshot exists, and this one is not more than 50%
   smaller than it.

Every complete, non-empty snapshot that is not too soon becomes the new
baseline, whether or not inference ran. A real large purge therefore delays
inference by one poll, rather than blocking it until the feed grows back.

When the guard passes, every `NOT is_cleared AND source_removed_at IS NULL`
row missing from the snapshot gets `source_missing_polls + 1`. A row that
reaches 2 gets `source_removed_at = fetched_at`. Planned and unplanned rows
are treated the same.

A chunk failure returns before inference runs, so a partly written snapshot
never marks anything. Text-changed events are published before inference,
so an inference failure (a 500 and a retry) cannot drop them.

Observability:

- `api_incident_removal_inference_total{outcome}`, one increment per POST.
  `outcome` is one of `applied`, `incomplete`, `empty`, `too_soon`,
  `no_baseline` or `shrink`. All are registered at 0 at startup.
- `api_incidents_marked_removed_total`: rows newly marked per poll.
- A log line per skip: a warning for `incomplete`/`empty`/`shrink`, info for
  the benign ones.
- Alert `DistantSignalIncidentRemovalStalled`: over 2h, no snapshot applied
  and at least one skipped as `incomplete`/`empty`/`shrink`. See
  [alerts.md](../../alerts.md#distantsignalincidentremovalstalled). There is
  no mass-removal alert: the shrink guard already stops the mass-removal
  failure mode, and the first deploy legitimately marks ~400 rows at once.

## `incident_history` on clear

`incident_changed` now includes `is_cleared`, so a flag-only clear writes an
`incident_history` row. The consumers were checked:

- the detail page's diff summary already prints `isCleared changed to ...`;
- the enricher's text recovery (`fetch_extracted_source_text`) takes the
  newest row matching a text hash, so an extra row with the same text is
  harmless;
- `scripts/incident-history-text-churn.sql` already counts non-text
  snapshots as `metadata_only`.

Becoming ended or un-ended writes no history row: it is not a change to the
feed's content.

## API contract (`docs/api-changelog.md`, 2026-10-06)

- `GET /public/incidents` rows and `GET /public/incidents/{id}` gain
  `sourceRemovedAt` (RFC3339 or `null`).
- `GET /public/incidents?state=active|cleared|ended`. An unknown value is a
  400.
- `cleared=true` keeps its meaning (`is_cleared`). `cleared=false` now means
  Active only and no longer includes ended rows. `cleared` and `state`
  together are a 400.

## Consumers of "active"

| Consumer | Change |
| --- | --- |
| Archive filter (`queries::search_incidents`) | `state`, as above |
| Frontend archive rows, filter, detail page | `IncidentStateBadge`, Status filter All/Active/Ended/Cleared, ended notice in place of "Currently affects" |
| Aggregator `load_incidents` | `AND source_removed_at IS NULL` |
| Enricher hourly sweep | `AND source_removed_at IS NULL` |

User decision 3: the enricher re-sweep in progress on 2026-10-06 was left to
run.

## Backfill

`scripts/backfill-2026-10-06-incident-source-removed.sql` is reviewed and not
yet run. Its dry run prints a count and a sample by default; `-v apply=1`
runs the UPDATE in one short transaction. It marks
`NOT is_cleared AND source_removed_at IS NULL AND fetched_at <
last_complete_at - 30 minutes` with `source_removed_at = fetched_at` and
`source_missing_polls = 2`. The expected scope was ~407 rows on 2026-10-06.

It is optional: the inference itself marks those rows about three complete
polls after both new images are live. The backfill only does it at once.

## Deploy

1. Migration and api. The api runs migrations at startup, and the old poller
   keeps working because its array is still accepted.
2. poller-incidents. Its first complete snapshot records the baseline, and
   the next two mark the stale rows (or run the backfill after the first).

Rollback: an older api ignores the new columns, and the older poller is
compatible with the new api.

## Testing

- Poller: completeness of clean, malformed, truncated, empty and unwrapped
  documents, and the serialized snapshot.
- api, DB-gated (`incident_removal::db_tests`): removal after 2 complete
  polls and not after 1, planned and unplanned alike; no inference on
  incomplete, empty, shrunken or too-soon snapshots; the shrink snapshot
  becomes the baseline; reappearance resets; a cleared row is never ended,
  and its clear writes history; the backfill's UPDATE block.
- api: `state`/`cleared` filters (query and route), `sourceRemovedAt`
  rendering, both POST body shapes.
- Aggregator and enricher sweep exclude ended rows (DB-gated).
- Frontend: `incidentState`, `IncidentStateBadge`, the archive's Status
  filter and URL restore, and the detail page's ended notice. The e2e seed
  gains an ended incident for the accessibility scan.
