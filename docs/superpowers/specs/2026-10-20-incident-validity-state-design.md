# Incident state from the validity windows (2026-10-20)

Status: implemented 2026-10-20. Amends
[2026-10-06-incident-source-removal-design.md](2026-10-06-incident-source-removal-design.md)
(the incident lifecycle table) and
[2026-09-12-incident-archive-design.md](2026-09-12-incident-archive-design.md)
(the `state` filter, and Decision 7: list rows still render no validity
periods, but the query now reads them).

## Problem (production, 2026-10-20 12:37 UTC)

An incident's "Active" badge, and the archive's `state=active` filter and
count, meant only "not cleared and still listed by the feed". Nothing looked
at the incident's own validity windows. RDM lists planned work days or weeks
ahead and never clears it, so almost every listed planned incident read as
active:

| Listed, uncleared incidents | Count |
| --- | --- |
| All | 891 (888 planned, 3 unplanned) |
| A window covers now | 42 (39 planned, 3 unplanned) |
| Every window still to come | **849** (all planned) |
| Every window finished | **4** (all planned) |

Every row had exactly one window. Six more rows the feed had stopped
listing also had future windows; they were already "Ended" and stay so.

Example: 2D8C8B610B654F93BEC3A7B377A9B2A0, "Buses replace trains between
Middlesbrough and Saltburn on Sunday 25 October", window
2026-10-24T23:00Z (00:00 BST) to 2026-10-25T23:59Z (23:59 GMT), `is_cleared`
false, listed, shown "Active" five days early. Its LLM-extracted period was
the same Sunday (another fix is correcting a wrong-year bug in some
extracted periods; this design does not depend on it).

The line pages were not affected: the aggregator already drops an incident
whose RDM validity does not cover now (`keep_reason`, rule 1), and the issue
list buckets line statuses by date (`frontend/lib/validity.ts`). So the
incident's page said "Active" while no line carried it.

## The rule

One function, `common::incident_validity::ValidityPhase::of`, places an
incident's feed validity windows relative to now:

- **active**: some window covers now, `from <= now < to` (an open end never
  finishes), or there are no windows at all (RDM's `ValidityPeriod` is
  mandatory; the aggregator has always read none as open-ended from now);
- **upcoming**: no window covers now and one starts later. This includes
  the gap between two windows: the next one is still to come;
- **ended**: every window has finished.

The incident's state (`api::data::queries::IncidentState::derive`) is the
first that applies:

1. `cleared`: RDM set `ClearedIncident`. A reopen clears the flag, and the
   incident is placed by its windows again (unchanged behaviour).
2. `ended`: the feed stopped listing it (`source_removed_at`), whatever its
   windows say (unchanged).
3. The window phase: `active`, `upcoming` or `ended`.

An incident that is still listed but whose windows have all finished is
"Ended", with "Its validity period ended <time>" where a removed one says
"No longer listed by the source since <time>". "Recently cleared" is
untouched: it lists RDM-cleared incidents only, and a cleared incident is
`cleared` here whatever its windows say.

**Time zone.** RDM's `StartTime`/`EndTime` carry a UTC offset, so every
bound is an exact instant and comparing instants needs no zone: a window
ending at 01:45 BST has ended by 01:30 GMT the same night. Europe/London
only matters for display, which the frontend already does
(`formatDateTime`). Tests cover both clock changes.

## Which periods to trust

The feed's structured windows decide the incident's state. The
LLM-extracted periods (`incidents.extracted_periods`) do not:

- the windows are RDM's own structured data, exact instants, on every row;
  the extracted periods are model output read from free text, with known
  date errors (the wrong-year bug being fixed separately) and a confidence
  that is often low;
- the specs give extracted periods a narrower job, all on the line pages:
  the severity hint, the rail-day cutoff's dated and undated exemptions,
  the "in effect only inside its schedule window" rule, and a purely
  future unplanned notice becoming a line's `upcoming` note
  ([2026-10-06-incident-line-evidence-design.md](2026-10-06-incident-line-evidence-design.md)
  §3, user decisions 5-7 and 10). None of them is "is this incident
  active", and each still applies to the line as before;
- the archive's filter has to be the same rule in SQL, and reading
  extracted periods there would mean porting their London-date reading
  (`period_bounds`) to SQL.

One known difference remains: an unplanned strike notice whose RDM window
started when it was published, but whose every extracted period is a
high-confidence future date, is "Active" on its own page while its lines
show it only as an `upcoming` note. RDM says the notice is live; the strike
is not. That is left as is (and tracked separately).

## Where it applies

| Surface | Change |
| --- | --- |
| `common::incident_validity` | New: `ValidityPhase`, `period_covers`, `next_start`, `ended_at` |
| Aggregator `keep_reason` / `period_covers_now` | Calls the shared rule (no behaviour change) |
| api `IncidentState` / `IncidentTiming` | `Upcoming` added; derived from the windows |
| api `incident_state_sql` | The same rule in SQL, for `state=` and `total`; a DB test checks it agrees with the Rust rule on every case |
| `GET /public/incidents`, `GET /public/incidents/{id}` | Additive `state`, `startsAt`, `validityEndedAt`; `state=upcoming`; see `docs/api-changelog.md` |
| Frontend badge, archive rows, detail page | "Upcoming" badge (planned tone) with its start; "Ended" with the window's end; the detail says when it starts or ended instead of "Currently affects" |
| Frontend Status filter | All / Active / Upcoming / Ended / Cleared |
| Line severity, line and station issue lists, notifications | Unchanged: already gated on the same windows (notifications fire on line severity transitions, which never include an incident outside its window) |

The frontend uses the api's `state`. Against an older api it applies the
same rule to `validityPeriods` where it has them (`lib/validity.ts`'s
`validityPhase`, the detail page), else reads uncleared, listed rows as
active, as before.
