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
   *Superseded in part by the misses follow-up below: places are counted
   per mention, the summary is resolved against the whole catalogue, the
   description fallback reads only its first paragraph, pass-through
   stations count as on a line, the hub rule gained a relative form, and
   lines sharing a significant part of a section count.*
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

## Misses follow-up (2026-10-06, second pass)

A misses investigation replayed the 30 days with the matcher above against
a timetable-derived reference: 40.4 affected line-days were missed while
the incident showed on other lines, and 44.8 wrong-line line-days were
shown. About 60% of the misses were sparse catalogue station lists: lines
that pass through a named section without stopping, or whose catalogue
does not list the section's ends. User decisions, all implemented:

1. **Veto on the summary only.** `excluded_keywords` are checked against
   the summary. 7CE5A87E ("Paddington and Heathrow Terminal 5 / Reading")
   described what Elizabeth line and Heathrow Express trains were doing,
   and each Heathrow line excludes the other's name.
2. **Umbrella lines.** `northern` and `cross-country` lose their operator
   brand keywords ("Northern services", "Northern Rail", "Northern
   Trains"; "CrossCountry", "Cross Country") and match by their stations.
   Line names ("Suffragette line") still match. The XC "Gloucester area"
   case that lost a match in the prototype is covered by pass-through:
   CrossCountry trains pass Gloucester between Cheltenham Spa and Bristol
   Parkway.
3. **Pass-through stations** (`lines/generated/pass-through.toml`, below).
4. **Resolver.** Places are counted per mention, so a multi-code name
   ("London St Pancras", "Heathrow Airport") is one place. A "/" lists a
   common-word place even after a number ("Terminal 5 / Reading").
   Aliases: "Heathrow", "Heathrow Airport", "Heathrow Terminals" (all three
   Heathrow stations), "Heathrow Airport Terminal 4/5", and a curated list
   of big-city names (Birmingham, Manchester, Glasgow, Bristol, Cardiff,
   Liverpool, Exeter, Southampton, Bath, Bradford, Wakefield, Edinburgh
   Waverley) mapped to their main terminals. "X area" resolves to X and,
   like "at/near/outside/through X", marks X as where the disruption is.
   A bare "London" counts only as an end of an explicit section in the
   summary ("between London and Stevenage", "between Stevenage and
   London"), as every London terminus: on each line, its own terminus. Not
   in "Stratford (London)" and not in descriptions. "Heathrow Express" is a
   brand, not a place.
5. **No operator-wide fallback once the summary names a place.** Summary
   places are resolved against every catalogue line. If no in-scope line
   holds any of them, the lines of any operator holding two or more are
   used (06724BDE, "XC" on "Grantham and Skegness", is the Poacher line),
   else none: the incident is local, so it is never shown operator-wide.
   Not when an in-scope line holds one of them (DB1DA9F3, "Reduced
   Thameslink service between London Kings Cross and Peterborough", stays
   on Thameslink). The description is read only when the summary names no
   place, and then only its first paragraph (what happened; the rest is
   ticket acceptance and travel advice).
6. **Relative hub rule.** With no two-place section, a place on 3 or more
   in-scope lines is dropped when another named place is on only one line
   (CD74FB58: Victoria, on four Southern lines, beside Eastbourne). The
   absolute rule (a place on more than 4 lines beside one on 4 or fewer)
   stays.
7. **Catalogue additions**, each checked against the CIF timetable
   (`schedule_calling_points_full`, 7/10/11 Oct 2026): `southern-west-london`
   (Watford Junction - Clapham Junction - East Croydon, as Southern runs it;
   no Southern train runs to Milton Keynes any more), `southern-marshlink`
   (Ashford International - Rye - Ore - Hastings), Clapham Junction on
   `southern-oxted-uckfield`, Ore on `southern-coastway-east`, Manchester
   Piccadilly on `tpe-north-scarborough` (the other TPE lines already list
   MAN/MIA where they serve them), the documented-but-missing Doncaster fork
   of `northern-wakefield-line`, and Bristol Parkway and Cheltenham Spa on
   `gwr-bristol-gloucester` (minus Stonehouse, which its trains never
   serve). New lines use their own segments, except Southern's West London
   line, which shares the Mildmay line's Clapham branch segment (the same
   five stations).
8. **Partial overlap**, below.
9. **Unresolved places are counted.** Each snapshot POST sets
   `distant_signal_api_incidents_without_resolved_place` (gauge, no
   labels) to the number of live incidents that are local (no network
   marker) but name no resolvable place, and logs each summary at debug
   level (the phrase is never a label). Fuzzy matching, region gazetteers
   and LLM place extraction are deferred.

### Pass-through stations

`scripts/generate-pass-through.py` (typed stdlib Python, `uv run`) reads
the schedule tables through `psql` (a `--database-url`, or `--psql
"kubectl ... exec -i distant-signal-postgres-0 -- psql ..."` against
production; every query runs in a `READ ONLY` transaction):
`schedule_calling_points_full` (calls and passes), operators from
`schedule_destination_departures` (passenger trains only) and `tiploc_crs`.
It takes the latest Wednesday, Saturday and Sunday the table holds.

- A train belongs to the lines of its operator whose stations its route
  reaches most often (ties: all), so a Crystal Palace metro train between
  Clapham Junction and London Bridge is not the Brighton Main Line's.
- For each pair of consecutive catalogue stations, the line's own trains'
  paths between them (no other station of the line in between), reduced
  to `stations` reference CRS codes. The most common path wins, or a
  fuller record of the same route run by at least a fifth as many trains
  (CIF records passes only at timing points, so a fast train's path omits
  stations a stopping train lists).
- A pair none of the line's trains runs between directly is a **break**
  (a branch boundary in the catalogue's station order).

The file is deterministic for the same data: a generated header,
`source_dates`, one `[lines.<id>]` table with `FROM-TO = ["CRS", ...]` per
leg, and a `[breaks]` table, `<id> = ["FROM-TO", ...]`. It lives in
`lines/generated/` so the `lines/*.toml` glob never reads it as a line.
2026-10-06 run (production, read-only): 512 pass-through stations on 84
lines, 295 breaks. (The prototype counted 778; this one is stricter: only
the line's own trains, and no leg across a branch boundary.)

`LineDefinition::from_dir` attaches it as `pass_through` (serde-`skip`:
never read from a line file, never serialised, never a stop, never sampled,
never in a segment). The matcher's `holds_place` counts its stations as on
the line; the station evidence maps a pass-through station to the line's
own stations either side, so affected stops and routes only ever name the
line's stops. A missing file means catalogue stations only (as before);
stale legs (ends no longer consecutive) are ignored and logged.
`line-catalogue-validator` (already a CI step, no database) fails on a
missing or unparseable file or an unknown line or CRS, and warns on stale
legs. Refresh: after each timetable change (December and May) and after
editing a line's stations; see `lines/SCHEMA.md`.

### Partial overlap (decision 8)

When some line holds two or more of the named places (a section), a line P
holding exactly one of them, p, also counts when the part of the section
it shares is significant for it. For each section line S holding p and
another named place q, take S's route (catalogue plus pass-through
stations, within one run between breaks) from p to q; P's **shared
stretch** is the start of it that P follows: each station P holds must be
next to the previous one on P's own route (stations P's data omits are
passed over). P counts when, for some S and q:

- **(a)** the shared stretch is at least **50%** of p..q in hops between
  consecutive stations (sharing only p itself is 0%); or
- **(b)** the shared stretch includes a **major junction** (a station 4 or
  more catalogue lines, any operator, hold) that the text puts the
  disruption at ("at X", "near X", "X area", in the summary or the
  description's first paragraph). Disruption at a junction delays every
  line through it; one merely named as a section end does not.

Worked examples (all in `matcher::tests`):

- **Bexleyheath / Maidstone East** ("Maidstone East and London Charing
  Cross", 38DE9D6A): no catalogue line holds both ends; Charing Cross (5
  lines) is a hub beside Maidstone East (1 line), so the relative hub rule
  leaves the Maidstone East line. The Bexleyheath line, which shares only
  the London end past Lewisham, is not shown.
- **Lewisham** ("Lewisham and Hayes"): the Hayes line alone. With "a
  signalling fault at Lewisham" in the description's first paragraph,
  Lewisham (4 lines) is a disrupted major junction, so the Bexleyheath,
  Dartford Loop and South Eastern Main lines through it count by (b).
  "Change at Lewisham" in a later paragraph does not localise.
- **Far North / Kyle at Inverness** ("Inverness and Kyle of Lochalsh",
  6A545EF3): the Far North line shares Inverness - Dingwall, under a third
  of the section: not shown. With "a points failure at Inverness"
  (4 lines), it is, by (b).
- **Clapham Junction** ("Clapham Junction and London Victoria"): Southern's
  West London line shares only Clapham Junction itself (0 hops): not shown,
  unless the disruption is at Clapham Junction (21 lines), as in 5930CC6A
  ("a fire ... in the Clapham Junction area"). "Raynes Park and Clapham
  Junction" (87C865A3): the South West Main, Portsmouth Direct, Alton and
  West of England lines run Clapham Junction - Wimbledon, two of the three
  hops, so count by (a); the Windsor lines, leaving at Clapham Junction, do
  not.
- **Leeds - York** (03552B44): TransPennine's Hull trains and Northern's
  Leeds - Selby trains share Leeds - Micklefield, over half of it: counted.
  TransPennine's Wakefield-route trains, sharing only Church Fenton - York,
  are not; neither is the Calder Valley line, which only touches Leeds.

### Replay (re-derived; 30 days to 2026-10-06)

Method (scratch scripts, not committed): every distinct
summary/description/operators text of the 636 unplanned incidents in
`incident_history`, kept or not by main's real `keep_reason` (a scratch
test inside the aggregator) at 15-minute steps, matched by the real Rust
matcher (a scratch example binary over the production `stations` names).
Line-days are time-weighted (a step is 1/96 line-day). Each change alone
is main plus that change (env-gated toggles in a scratch copy; catalogue
changes as composed `lines/` directories).

Reference ("affected"): for each text, its places (an independent
longest-match gazetteer with generous aliases); the incident operators'
passenger trains on 7/10/11 Oct (any operator's when theirs run none
there) whose path hits two section ends (or the one place); a line is
affected when such a train is one of its trains (best-fitting line, or
reaching 3 of its stations with over half the stretch on its route) and the
line's route (catalogue plus pass-through) holds the ends. Lines whose
trains share only part of the section are "partial" and ungraded, except
71 hand-judged (incident, line) pairs: every partial-overlap candidate at
the most liberal setting (25%, N=3), judged from the description's lead,
and CD74FB58's Brighton Main Line (graded partial, per decision 6).
Network-wide notices and texts with no place are not graded.

| Variant | Missed (shown elsewhere) | Wrong-line | Mean lines/incident |
| --- | --- | --- | --- |
| main (base) | 41.7 | 41.4 | 2.47 |
| 1 veto on summary alone | 65.0 | 29.2 | 2.48 |
| 2 umbrella keywords alone | 43.8 | 31.5 | 2.39 |
| 3 pass-through alone | 29.1 | 41.1 | 2.61 |
| 4 resolver fixes alone | 38.1 | 34.8 | 2.50 |
| 5 no operator-wide fallback alone | 40.4 (+1.2 hidden) | 35.9 | 2.44 |
| 6 relative hub alone | 41.9 | 41.4 | 2.46 |
| 7 added lines alone | 36.0 | 36.0 | 2.47 |
| 8 partial overlap alone | 36.6 | 43.5 | 2.54 |
| 8b description first paragraph alone | 41.1 | 62.5 | 2.65 |
| **all together** | **2.7** | **19.6** | **2.54** |

Leave one out (all others on; missed / wrong): without 1, 16.2 / 19.6;
2, 2.7 / 29.3; 3, 15.7 / 21.2; 4, 4.3 / 25.4; 5, 2.7 / 19.6; 6, 2.3 /
20.0; 7, 7.5 / 19.4; 8, 7.5 / 18.1; 8b, 2.7 / 19.8. The changes interact:
the veto fix alone loses the GW lines on the long-running 7CE5A87E (the
Heathrow pair holds two places while "/ Reading" is unread), which the "/"
fix restores; the first-paragraph fallback alone resolves a lone hub where
the full description had resolved a section; decision 5 changes nothing
measurable once the added lines exist (its cases were Southern's missing
lines).

Partial-overlap sensitivity (all other changes on; missed / wrong):

| share \ N | 3 | 4 | 5 | 6 | 8 | (b) off |
| --- | --- | --- | --- | --- | --- | --- |
| 25% | 1.5 / 23.1 | 1.6 / 23.0 | 1.7 / 22.9 | 1.7 / 22.7 | 1.7 / 22.7 | 4.3 / 22.4 |
| 33% | 2.4 / 21.3 | 2.6 / 21.2 | 2.7 / 21.1 | 2.7 / 21.0 | 2.7 / 21.0 | 5.3 / 20.7 |
| **50%** | 2.5 / 19.7 | **2.7 / 19.6** | 2.8 / 19.5 | 2.8 / 19.3 | 2.8 / 19.3 | 6.0 / 19.0 |
| 67% | 3.8 / 19.0 | 4.0 / 18.9 | 4.1 / 18.8 | 4.2 / 18.7 | 4.2 / 18.7 | 7.4 / 18.4 |
| (a) off | 3.9 / 18.8 | 4.1 / 18.7 | 4.2 / 18.6 | 4.2 / 18.4 | 4.2 / 18.4 | 7.5 / 18.1 |

Chosen: 50% and N = 4. The result is flat in N (the junction rule fires on
few incidents); 4 is the smallest N that keeps Lewisham, Inverness and
East Croydon (4 lines each) as junctions, while N = 3 would add 156 more
stations. Below 50% the share rule adds wrong lines faster than it removes
misses. Remaining wrong-line days are mostly catalogue quirks
(`gwr-transwilts` lists Paddington and Reading; the `northern` umbrella's
fragments) and the reference's own limits.

Spot checks (10 random incidents, final matcher): Sittingbourne (Chatham,
High Speed, Sheerness: correct), Hastings - Battle (Hastings line:
correct), Cambridge - Royston (GN King's Lynn, Thameslink Cambridge:
correct), Birkenhead Hamilton Square (Wirral: correct), Belmont (Epsom
Downs: correct), Fenchurch Street - West Ham (c2c: correct), Bidston -
Wrexham Central (Borderlands: correct), Preston - Blackburn (East
Lancashire: correct, though Northern's Blackpool - York trains also run
it), through Wakefield Kirkgate (Hallam, Pontefract, TPE Wakefield; the
`northern` umbrella no longer added: correct), Par - Newquay (Atlantic
Coast: correct).

### Deploy (follow-up)

No migrations. api and aggregator in any order: both load
`lines/generated/pass-through.toml` from the image's `lines/` (the
Dockerfiles copy the directory recursively); an older image ignores the
file. The new gauge appears with the new api. Optional: re-run
`backfill_incident_lines` for archived rows. Rollback: previous images.
