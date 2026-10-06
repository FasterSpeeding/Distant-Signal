# Incident line evidence and the unplanned cutoff (2026-10-06)

Status: implemented 2026-10-06, live (no shadow mode, user decision).
Amends [2026-09-05-incident-line-matching-false-positive-design.md](2026-09-05-incident-line-matching-false-positive-design.md)
(its "Tier 1 is dead" non-goal is now built),
[2026-07-16-stale-incident-handling-design.md](2026-07-16-stale-incident-handling-design.md)
(the cutoff's anchor and exemptions) and
[2026-08-21-multi-period-extraction-design.md](2026-08-21-multi-period-extraction-design.md)
(how period dates are read). Builds on
[2026-10-06-incident-source-removal-design.md](2026-10-06-incident-source-removal-design.md).

## Problem (production study, 30 days to 2026-10-06)

**Line matching.** RDM's Knowledgebase feed has no structured station
field, and `poller-incidents` leaves `affected_stations` empty, so the
matcher's station tier never fired. Of 638 unplanned incidents, 591 matched
operator-only and showed "Minor Delays (operator-wide report)" on every line
of their operator; 28 matched nothing (`LN`, `WM`, `ZN` codes no catalogue
line uses); 17 matched a keyword. About 89% of the first-day operator-wide
line-days were on unaffected lines. Yet about 98% of incidents are local and
name their stations in the summary, and only 1% (6) are network-wide.

**The cutoff.** An unplanned incident is shown only until the next 02:00
Europe/London rail-day boundary after `first_seen_at`, unless a
high-confidence recurring schedule window exempts it. Over 30 days, 46
unplanned incidents outlived the cutoff: 16 genuinely long-running, 7
reopened ids (RDM reuses ids: B852BEF3, Purley - Gatwick Airport, was
cleared and reopened on 11, 12 and 13 Sep, and the cutoff hid it from the
moment it reopened), 2 future strikes, 20 short overnight recovery notices
(the cutoff is right) and 1 advisory.

**Extraction dates.** Zero-length periods (a strike "on Sunday 11 October",
`from_date == to_date`) never became `Active`; some strike days were written
at UTC midnight instead of London midnight; the enricher read relative
dates against `first_seen_at` rather than when the text appeared
(6C20E627's "today", set on 16 Sep, was extracted as 9-10 Sep); and "Normal
timetable expected to resume" was labelled an ongoing period.

## Decisions (user, 2026-10-06)

1. Evidence-based matching everywhere, including an incident's first day.
2. An unplanned incident shows on the lines with evidence (a station or
   segment hit, a closed section, a keyword or brand, resolved places). On
   all its operator's lines only with a network-scope marker, or as a
   last-resort fallback when no place resolves. Existing severity caps
   stay, including Minor Delays for operator-wide display.
3. `LN`/`WM` are the catalogue's `LM`; `ZN` resolves across all operators
   via its text. Overground brand keywords and station aliases added.
4. The cutoff re-anchors on a new `active_since`.
5. A dated exemption: an `Active`, ongoing, high-confidence period with a
   stated end or a schedule window.
6. Undated long-term notices stay while listed, capped at Reduced Service.
7. An incident kept only by an exemption is in effect only while its
   exempting period is inside its window.
8. Date fixes: zero-length ranges are whole London days; London-aware day
   boundaries; the enricher's reference date is when the current text
   appeared.
9. "Ended" (`source_removed_at`) overrides everything.
10. Upcoming strikes are a separate note on the line, not a severity.

## Design as built

### 1. The station resolver (`common::station_resolver`)

`StationGazetteer` holds every `stations` row's normalised name (the
`no_trains` normalisation: lowercase, apostrophes dropped, parenthesised
qualifier dropped, "&" as "and") plus:

- **derived aliases**: the name without a trailing county word ("Seaford
  Sussex" -> "Seaford"); a London terminus without "London" when the rest
  is two or more words or a known terminus word ("Victoria", "Waterloo",
  "Euston", ...), never "Bridge"/"Fields"/"Road";
- **hand-written aliases** (`EXTRA_ALIASES`): "James Street", "Lime
  Street", "St Pancras"/"London St Pancras", "Hull Paragon", "Milton
  Keynes".

`stations_in(text, in_scope)` strips HTML, tokenises like the
normalisation, and scans for the **longest** name at each position (so
"Purley Oaks" is not Purley, and "Reading West" never falls back to
Reading even when only Reading is in scope). A name must start with a
capital letter in the text. Guards:

- a name that is also a common word ("Reading", "March", "Hope", "Deal",
  ...) counts only after a place word ("between", "and", "at", "to",
  "from", "via", ...) or a "/", and not next to a number ("from March
  2027");
- a name followed by "line", "lines", "branch", "route", "main line" or
  "Trains" is a line or operator ("Brighton Main Line", "Hull Trains",
  "Victoria line").

### 2. Matching (`common::matcher::lines_affected_by`)

1. Operator codes go through `effective_operator` (`LN`, `WM` -> `LM`).
2. **Scope.** If every code is `ZN` or unknown to the catalogue (or there
   are none), places resolve across every line and the incident's operators
   no longer contradict a keyword hit. Otherwise places resolve only to
   stations of the incident's operators' lines.
3. **Places.** The summary is resolved; the description only if the
   summary names no place in scope (descriptions add ticket acceptance and
   diversion routes). Per line, the resolved places it holds:
   - if any line holds two or more, only lines holding two or more count
     (a section: "between Purley and Gatwick Airport");
   - otherwise every line holding one counts (a hub: "at Clapham
     Junction"), except that a place on more than 4 in-scope lines is
     dropped when a more local place was also named ("Ore / Eastbourne and
     London Victoria" is the Eastbourne line, not every line into
     Victoria).
   The result feeds the existing station tier exactly like
   `affected_stations` (segment classification unchanged).
4. **Operator-wide only on a marker or as a fallback.** With a network-scope
   marker every operator-only match stays. Otherwise, if any place resolved,
   every operator-only match is dropped. If none resolved, the pre-existing
   per-operator rule applies (a keyword hit on one of an operator's lines
   drops that operator's other operator-only matches).

`has_network_scope_marker`: industrial action ("industrial action", "strike
action", "on strike", "strike day(s)", "RMT/ASLEF/TSSA/Unite strike") in
the summary or description; in the summary only, "across the ... network",
"network-wide", "Intercity routes", "reduced ... timetable", "all ...
services" and "reduced <words> service" when no place word follows and (for
"reduced") no line is named. A bridge or lightning strike is not industrial
action. Summary-only for the loose markers because local incidents'
descriptions say things like "a severely reduced service is running
between X and Y" (6C20E627, found by the replay below).

**Where it runs, and why.** At match time, in the one shared function, in
every caller: the api's ingest (`incidents.affected_lines`, the archive's
Line filter), the `backfill_incident_lines` binary, and the aggregator (live
statuses and `upcoming`). Each loads the `stations` table itself (the api
per snapshot POST, ~2,600 short rows every 5 minutes; the aggregator per
cycle, as it already did for `no_trains`). Rejected: resolving once at
ingest into a new `derived_stations` column. It would have saved the
aggregator a scan, but the archive, the live statuses and the backfill
would then depend on when a row was last written, a `lines/*.toml` or alias
change would need a re-ingest before the live pages saw it, and custom
lines (merged in by the aggregator) would get a second code path. Both
loaders fail open to an empty gazetteer, which is exactly the
pre-2026-10-06 matcher.

`no_trains` shares the gazetteer (names and aliases) for its section ends.
Since the section's ends are now usually the line's station evidence, a
closed section caps severity at the section's own severity under every
scope (not just weak ones), names its whole extent in `affectedStops`, and
suppresses the "shared trunk" annotation (the section's own "(part of the
line)" says what it covers).

### 3. The cutoff (`aggregator::aggregation::keep_reason`)

`incidents.active_since` (`20261006130000`, backfilled by `20261006130100`)
is stamped by the api upsert on insert, on a reopen (`is_cleared` true to
false) and on a summary/description change while uncleared. Being listed
again after an "Ended" spell does not re-arm it (the nightly purge and
relisting would renew stale incidents daily); relisted text that is really
new does. The aggregator reads `COALESCE(active_since, first_seen_at)`.

An incident contributes a status when its RDM validity covers now, and:

| Case | Kept as | Shown |
| --- | --- | --- |
| Planned | `Current` | as before |
| Unplanned, every period high-confidence and not started | not kept | only as `upcoming` |
| Unplanned, before the next 02:00 after `active_since` | `Current` | as before |
| Past it, an `Active` ongoing high-confidence period with a `to_date` or a schedule window | `Dated` | in effect only inside its window |
| Past it, an `Active` ongoing high-confidence open-ended period, text says "until further notice" (or similar) | `Undated` | capped at Reduced Service, "(long-running notice)" |
| Otherwise | not kept | falls back to inferred / Good Service |

The undated case needs the long-term phrase because an `Active`, ongoing,
high-confidence period with no dates is the ordinary single-fact extraction
of every live incident; without the phrase it would bring back the "SWR
forgot about it" failure the cutoff exists for.

**Not in effect.** A `Dated` status outside its schedule window (a Sunday
under a Monday-Saturday window) gets a validity starting at the window's
next start, `is_now = false`, so `in_effect_now` is false: no LDBWS or
full-coverage escalation attaches, and the line also shows its own inferred
status when live data shows disruption. `apply_extraction` already demotes
it to Minor Delays with "reported active HH:MM-HH:MM only".

"Ended" beats all of it: `load_incidents` never loads a row with
`source_removed_at`.

### 4. Period dates (`period_bounds`)

- A bound at exactly 00:00 UTC is a calendar date and is read as that
  date's Europe/London midnight (in BST, 23:00Z the day before).
- `to_date <= from_date` is the whole London day of `from_date` (23, 24 or
  25 hours).
- A period whose scope says normal service/timetable resumes is dropped at
  parse time (it would otherwise escalate, exempt or announce from its
  start date).

Not changed: an inclusive end day written as that day's own midnight
(7933A3FB, "until at least Monday 2 November" -> `2026-11-02T00:00Z`) still
ends a day early. The enricher's prompt asks for the following midnight;
reading every midnight `to_date` as inclusive would over-extend every
correctly-extracted period by a day.

### 5. Upcoming (`aggregation::upcoming_by_line`, `line_status.upcoming`)

For every live unplanned incident (not cleared, not ended; the cutoff does
not apply), every period that is ongoing, high-confidence, not started and
has a `from_date`, when the text mentions industrial action or the period
starts within 14 days. Attached to the lines the incident matches, soonest
first, at most 5 per line, written to `line_status.upcoming` every cycle
(no `line_status_history` row of its own). The api renders it as `upcoming`
on `GET /Line/Mode/{mode}/Status` and `GET /Line/{ids}/Status`
(docs/api-changelog.md); the frontend shows the soonest on the line card
and all of them on the line page, linked to the incident.

### 6. The enricher's reference date

`fetch_incident_state` returns, as `reference_date`, the earliest
`incident_history.recorded_at` of the latest unbroken run with the current
summary and description (A -> B -> A reads from the second A), falling
back to `first_seen_at`. **No `model_version` bump**: existing extractions
stay until their text next changes (the enricher skips text it already
extracted). The only rows that differ are incidents whose text changed days
after first being seen; a stale date there fails safe (an elapsed period
exempts nothing). If wanted, a one-off targeted re-extraction is to clear
`source_text_hash` on the live rows whose current text appeared more than a
day after `first_seen_at`; not done here (no production writes).

## Replay (30 days of production data, before vs after)

Old matcher and cutoff vs new, over the 638 unplanned incidents and their
history in 15-minute steps, against the 2026-10-06 study's independent text
classifier as the reference for "the lines it really affects" and its dated
text heuristic for "is it live" (so "after" is graded by a similar, not
identical, rule):

| | before | after |
| --- | --- | --- |
| Matched by station evidence / keyword / network marker / fallback / nothing | 0 / 17 / 2 / 591 / 28 | 620 / 5 / 6 / 6 / 1 |
| Mean lines per incident | 18.8 | 2.4 |
| Line-days shown | 2,413 | 698 |
| Wrong-line line-days | 2,107 | 3.7 |
| Affected line-days missed (of 476 live) | 170 | 94 |
| ... of which while the incident was hidden | 158 | 58 |
| ... of which while shown on other lines | 12 | 36 |

"Missed while hidden" after is an upper bound: the replay has extractions
only for each incident's current text, so earlier texts get no exemption.
"Missed while shown" rose because the new rule is deliberately narrower
than operator-wide (the hub rule and the two-place rule are
under-inclusive in a few cases, e.g. CD74FB58 now shows on the Coastway
East line only).

## Testing

- `common::station_resolver`: longest match, aliases, London short forms,
  common words, line names, HTML, operator aliases, network markers.
- `common::matcher` (real catalogue and a snapshot of the production
  `stations` names, `crates/common/testdata/station-names.csv`):
  Scarborough-Hull, Pontypridd-Cardiff Bay, Leeds-York, Purley-Gatwick,
  Woking-Brookwood, ZN (Uckfield, Ore/Eastbourne), LN/WM, Overground brands,
  aliases, hub fan-out, "Reading the timetable", description fallback,
  network markers, operator-wide fallback.
- `aggregator::aggregation`: reopened id (B852BEF3), stale text expiring at
  02:00 and a text edit re-arming (147D1B86), each exemption condition, the
  strike days (zero-length, UTC midnight, 25 Oct), `period_bounds`, a
  Sunday under a Monday-Saturday window (not in effect, no escalation,
  inferred fallback), undated capped at Reduced Service, the resumption
  period, upcoming notes, and the network/local/fallback rule end to end.
- DB-gated: the api upsert's `active_since` transitions and the backfill
  migration; the aggregator's anchor COALESCE, "Ended" beating a strike-day
  exemption, and `line_status.upcoming`; the enricher's reference date
  (6C20E627 pattern).

## Deploy

1. Migrations run with the api (all three are short: two catalog-only
   ALTERs and a bounded UPDATE of the ~20 live unplanned rows).
2. api, aggregator, enricher and frontend in any order. An older
   aggregator ignores `active_since`/`upcoming` (`upcoming` stays `[]`); an
   older api ignores both columns; the frontend treats a missing `upcoming`
   as none.
3. Optional: re-run `backfill_incident_lines` so archived rows'
   `affected_lines` use the new matcher (live rows are recomputed every
   poll).

Rollback: the previous images ignore the new columns; the migrations need
no rollback.
