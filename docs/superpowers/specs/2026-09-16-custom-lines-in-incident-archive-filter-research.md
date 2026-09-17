# Custom Lines as an Incident-Archive Filter — Feasibility Research

**Status: research only, not an approved design, and deliberately not a
single recommendation.** Written in the shape of
`docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`
and `docs/superpowers/specs/2026-09-01-stanox-crs-live-reference-data-research.md`
— investigate, state what was actually found with citations, lay out the
real tradeoffs, and stop short of committing where the evidence genuinely
supports two different answers. A follow-up `-design.md` would be the
deciding half; this is not it.

> **Update, 2026-09-17 — Finding 2's defect is fixed; two citations moved.**
> Everything below records the codebase as it was on 2026-09-16 and is left
> unedited, but two things a reader acting on it now needs to know:
>
> - **The dead catalogue `line` filter (Finding 2) has been fixed**, along
>   the lines of option **(e)** in "The matching-semantics question" —
>   persist the matcher's verdict and join to it — and so the "file it as a
>   bug" item under "Do this regardless" is closed. Two details differ from
>   this document's sketch of (e), both deliberately: the verdict is stored
>   as an `incidents.affected_lines TEXT[]` column rather than an
>   `incident_line_matches` table (no `scope`, no retention policy, no
>   cascade), and it is written by **`api` at ingest**
>   (`queries::upsert_incidents`) rather than by the aggregator per cycle.
>   The latter is what dissolves the backfill objection raised under
>   Approach B: the aggregator only ever sees `WHERE NOT is_cleared`, so an
>   incident cleared before the feature shipped could never get a row,
>   whereas the ingest path sees every incident the poller sends, cleared
>   included, and a one-off `backfill_incident_lines` re-matches the rest
>   (`docs/incident-affected-lines-backfill.md`). Note this does **not**
>   deliver the custom-line filter this document is about — `affected_lines`
>   holds catalogue ids only, and the privacy work in Finding 5 is untouched
>   and still outstanding.
> - **`crates/aggregator/src/matcher.rs` is now `crates/common/src/matcher.rs`**
>   (and `segments.rs` likewise), moved so `api` can run the same matcher.
>   The line numbers cited throughout still point at the right code, in the
>   new file. Finding 3's analysis of how a custom line passes through the
>   matcher is unaffected: custom lines reach only the `OperatorOnly` tier,
>   exactly as described.

## Question being researched

Can a user filter the incident archive (`/incidents`,
`GET /public/incidents`) by one of **their own private custom lines**, or
by a custom line **shared into a group they belong to** (per
`docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md`),
rather than only by the public catalogue/TfL lines it offers today?

The brief framed this as mostly a plumbing question ("resolve the custom
line's stations server-side instead of the catalogue line's"). The
investigation says it is not: the feature as literally specified would
ship a filter that **cannot match anything**, and the surrounding area
contains a live, currently-shipped privacy leak that any work here will
walk straight into.

## Method

- Read the archive's actual SQL and route parameters
  (`crates/api/src/data/queries.rs`, `crates/api/src/routes/incidents.rs`)
  rather than the design doc's description of them.
- Read the custom-line data model end to end (`crates/common/src/lib.rs`,
  `crates/api/src/data/custom_lines.rs`,
  `crates/aggregator/src/aggregation.rs`,
  `frontend/app/lines/CustomLineForm.tsx`).
- Read the matcher that decides which lines an incident affects
  (`crates/aggregator/src/matcher.rs`) and traced whether custom lines
  pass through it.
- Traced every existing custom-line read gate
  (`readable_custom_line_ids` and its four production call sites) to see
  which gates a new filter could reuse, and — more usefully — which
  comparable read path was *missed*.
- **Measured the production deployment** at `https://ds.cursed.solutions/`
  (2026-09-16) rather than assuming the data shape. This is where the
  decisive finding came from.

---

## Finding 1 — what a "line filter" means on this route today

`queries::search_incidents`
(`crates/api/src/data/queries.rs:2069-2136`) takes **no line concept at
all**. Its only two set-shaped filters are plain Postgres array-overlap
predicates against two columns on `incidents`:

```sql
WHERE ($1::text[]      IS NULL OR operators && $1)
  AND ($2::text[]      IS NULL OR affected_stations && $2)
```

(`crates/api/src/data/queries.rs:2089-2090`). The function's own doc
comment is explicit that it "has no knowledge of line catalogues at all"
(`queries.rs:2063-2067`).

The line concept lives one layer up, in the route
(`crates/api/src/routes/incidents.rs:203-215`):

```rust
Some(line_id) => {
    let Some(line) = app.config.lines.iter().find(|l| l.id == line_id) else {
        return Err((StatusCode::BAD_REQUEST, "unknown line".to_string()));
    };
    Some(line.stations.iter().map(|s| s.crs.clone()).collect())
}
```

So: **`?line=` is resolved server-side into that catalogue line's CRS
list, and then applied purely as a station-overlap filter.** An incident
is not tagged with a line id anywhere in `incidents`; it carries an
`operators: text[]` (ATOC codes) and an `affected_stations: text[]` (CRS
codes), and nothing else a filter could key on.

Two properties of the route worth carrying forward:

- `IncidentSearchParams` is `#[serde(deny_unknown_fields)]`
  (`routes/incidents.rs:52-53`), so a new parameter must be added to the
  struct explicitly; a misspelled one is a `400`.
- An unresolvable `line` id is a `400 "unknown line"`, **uniformly**, for
  every caller (`routes/incidents.rs:210`). A `custom-…` id today gets
  exactly the same `400` an entirely made-up id gets. That uniformity is
  itself a (currently accidental) privacy property — see Finding 5.

The archive design named this scoping deliberately:
`docs/superpowers/specs/2026-09-12-incident-archive-design.md:174-179`
says resolving `line` against `app.config.lines` only "keeps this new
route fully public with no session/ownership check needed, and sidesteps
any question about whether an unauthenticated caller could use an
incidents search as an oracle to learn something about a private custom
line's station list." Filtering by a private custom line is listed as an
explicit non-goal at `:735-740`. **This research is the request to
revisit that non-goal, so its stated rationale is exactly the thing that
has to be answered, not inherited.**

---

## Finding 2 — `affected_stations` is always empty, so today's `line` filter is inert

This is the finding that reframes everything else.

`IncidentMessage.affected_stations` carries an in-repo comment saying so
outright (`crates/common/src/lib.rs:585-586`):

```rust
pub operators: Vec<String>, // ATOC codes, flattened from Affects.Operators.AffectedOperator[].OperatorRef
pub affected_stations: Vec<String>, // left empty by pollers — no CRS field exists in the Incidents schema, only free-text RoutesAffected
```

The one poller that produces these messages hard-codes it
(`crates/poller-incidents/src/schema.rs:106`):

```rust
affected_stations: vec![],
```

and the one production writer of the table binds that value straight
through (`crates/api/src/data/queries.rs:83-220`, `upsert_incidents`; the
`INSERT INTO incidents … ON CONFLICT … DO UPDATE SET affected_stations =
EXCLUDED.affected_stations` upsert). Every other `INSERT INTO incidents`
in the repo is test-fixture seeding, inside a `#[cfg(test)]` module
(`crates/aggregator/src/queries.rs`, `crates/api/src/routes/incidents.rs`,
`crates/api/src/data/queries.rs` ×2, `crates/enricher/src/main.rs`). The enricher writes
`extracted_periods`/`source_text_hash`/`extraction_model_version`; it does
**not** backfill CRS codes.

**Measured against production, 2026-09-16:** the **entire** archive was
paged through `GET /public/incidents?limit=200` to exhaustion — 8 pages,
**1507** incidents, spanning `2026-09-03T03:43Z` → `2026-09-16T22:13Z`
(the deployment holds nothing older; the last page returned a null
cursor):

| | count |
|---|---|
| incidents with a non-empty `operators` | 1507 / 1507 |
| incidents with a non-empty `affectedStations` | **0 / 1507** |

A spot-check of a single incident detail
(`/public/incidents/815A2D9F55D3477982B19529C65E545D`) shows the same:
`"affectedStations": []`, `"operators": ["TL"]`.

**And the filter itself was probed end to end, live.** Every one of the
**109** catalogue line ids returned by `GET /public/lines` was submitted
as `GET /public/incidents?limit=1&line={id}` with no date bound (i.e. "All
time"):

| | count |
|---|---|
| catalogue lines probed | 109 |
| lines returning **at least one** incident | **0** |
| non-JSON / error responses | 0 |

Each returned `{"nextCursor":null,"results":[]}` — a clean `200` with an
empty array, indistinguishable from "this line had no incidents ever."
This is not an inference from the schema; it is the shipped filter,
measured against the shipped data, failing for every line the UI offers.
(A made-up id such as `line=west-coast-main-line` returns `400 "unknown
line"` instead, confirming the route's uniform-rejection behavior
described in Finding 1.)

**Independently corroborated.** A sibling document written the same day
against a completely different brief —
`docs/superpowers/specs/2026-09-16-tfl-incident-archive-design.md`, §1c
("The gap the assumption does not anticipate: the Line filter matches
nothing, for any line") — traced the identical code path and ran its own
live probes (`?line=elizabeth-line`, `?line=overground-mildmay`, both
empty), reaching the same verdict and calling it "a pre-existing,
network-wide defect of the shipped archive, not a TfL problem." Two
independent investigations, same conclusion; this is not an artefact of
how this document sampled the data.

Confirmed a third time through the real UI (headless Chromium against
production, same date): the `Line (optional)` select offers exactly **109**
options — the catalogue set, with no TfL and no custom entries for an
anonymous visitor — and selecting the first of them ("c2c (London, Tilbury
& Southend line)") and pressing Search renders **"No incidents match these
filters."** under the form. A user of this feature, today, cannot tell that
apart from a genuinely quiet line.

### Three consequences

1. **The catalogue `line` filter that exists today returns zero rows for
   every line, always** — measured, not inferred (109/109 above).
   `affected_stations && $2` against an
   always-empty left operand is always false. The UI's honest-sounding
   hedge — "a station-overlap approximation, not a real line match… can
   miss incidents that only matched a line by keyword or shared operator"
   (`frontend/components/IncidentSearchForm.tsx:383`) — understates it by
   a wide margin. It does not "miss some"; it misses all. This is a
   pre-existing bug the archive's own integration tests could not catch,
   because they seed `affected_stations` by hand
   (`routes/incidents.rs:722-744` seeds `&["WOK"]`) and so test a data
   shape production never produces.
2. **Copying that mechanism for custom lines would ship a second dead
   filter.** "Resolve the custom line's `stations` to CRS codes and pass
   them as `affected_stations`" — the obvious, minimal implementation —
   is guaranteed to return an empty result set for every custom line.
   Worse than useless: it looks like "this custom line had a quiet
   month," which is a *wrong answer presented confidently*, not a blank.
3. **Any design here must first decide whether it is also fixing the
   catalogue filter.** Shipping a working custom-line filter next to a
   broken catalogue-line filter, both labelled "Line", would be a worse
   state than today.

---

## Finding 3 — what a custom line actually is

`common::CustomLine` (`crates/common/src/lib.rs:1156-1174`) is five
fields:

```rust
pub struct CustomLine {
    pub id: String,                          // "custom-my-commute" (slugify(), custom_lines.rs:20-36)
    pub name: String,
    pub operators: Vec<String>,              // ATOC codes
    pub stations: Vec<String>,               // ordered CRS codes
    pub headcode_prefixes: Vec<String>,      // train-matching only
    pub destination_crs_filter: Vec<String>, // train-matching only
}
```

`CustomLineForm.tsx` confirms that user-facing shape exactly: a name, an
operators `TagsInput` (ATOC codes,
`frontend/app/lines/CustomLineForm.tsx:144-156`), a CRS station list
(`:157-202`, with a hard "at least 2 stations" gate at `:102`), and two
advanced fields behind a collapse (`:206-223`).

So a custom line **is** a `(set of operators, ordered set of CRS codes)`
pair, plus two train-matching extras irrelevant to incidents. On the face
of it that is a natural join with `incidents.operators` and
`incidents.affected_stations` — which is what made the brief's premise
reasonable. Finding 2 is what breaks it.

### Custom lines already go through the incident matcher

`From<CustomLine> for LineDefinition`
(`crates/common/src/lib.rs:1176-1207`) upgrades a custom line into a
full `LineDefinition` with `category: "custom"`, its CRS list expanded
into `Station` rows, and — critically — **empty** `match_keywords`,
`excluded_keywords`, `severity_overrides` and `exclusive_segments`. The
aggregator then merges those into the catalogue on every poll cycle
(`crates/aggregator/src/aggregation.rs:36-45`,
`crates/aggregator/src/main.rs:193-194`) so that, in that function's own
doc comment (`aggregation.rs:30-35`), "the rest of the
pipeline — matcher, segment registry, LDBWS inference — treats them
identically to catalogue lines."

That means the repo **already has** an authoritative answer to "which
incidents affect this custom line": whatever
`matcher::lines_affected_by` says. Feed that through the three tiers
(`crates/aggregator/src/matcher.rs:88-185`) with Finding 2 applied:

- **Tier 1 — `StationHit`/`SharedSegment`/`ExclusiveSegment`** requires
  `incident.affected_stations` ∩ `line.stations` ≠ ∅
  (`matcher.rs:100-105`). Always empty in production ⇒ **never fires, for
  any line, custom or catalogue.**
- **Tier 2 — `KeywordOnly`** requires `line.match_keywords`
  (`matcher.rs:106-111`). A custom line's is `vec![]` by construction ⇒
  **can never fire for a custom line** (it does fire for catalogue lines).
- **Tier 3 — `OperatorOnly`** requires `line.operators ∩
  incident.operators ≠ ∅` (`matcher.rs:171-182`).

**Therefore: for a custom line, the app's own matcher reduces exactly to
operator overlap, and the user's carefully-picked station list is
completely inert for incident purposes.** The stations matter for LDBWS
sampling and train matching; they contribute nothing to incident
attribution today.

And even that operator match is conditionally deleted. `lines_affected_by`
(`matcher.rs:70-83`) drops an `OperatorOnly` match when some *other* line
sharing an operator code got a more precise match anywhere in the same
incident:

```rust
out.retain(|m| {
    m.scope != MatchScope::OperatorOnly
        || !m.line.operators.iter().any(|op| precise_operators.contains(op.as_str()))
});
```

Since catalogue lines *do* have keywords and custom lines never do, a
custom line on `SW` loses its match to any SWR catalogue line that
keyword-hits the same incident. That suppression is the deliberate output
of
`docs/superpowers/specs/2026-09-05-incident-line-matching-false-positive-design.md`
and is the mechanism behind the app's own tagline — "an incident is only
ever flagged on the lines it actually affects"
(`frontend/app/layout.tsx:50`). A custom-line archive filter that ignores
it would contradict a stated product principle *and* disagree with what
the home page shows for the very same line.

---

## Finding 4 — the "which incidents affected this custom line" relation already exists, but not where the archive can reach it

The aggregator persists its per-line verdict:
`write_line_status` (`crates/aggregator/src/queries.rs:434-483`) upserts
one row per line — **custom lines included**, since they are in the merged
map — into `line_status`, with each incident-derived status carrying
`disruption.source = "knowledgebase-incident-{id}"`
(`crates/aggregator/src/aggregation.rs:156`). It also appends a
`line_status_history` snapshot when the statuses changed (`queries.rs:476`).

`queries::lines_currently_reporting_incident`
(`crates/api/src/data/queries.rs:1991-2005`) already runs the inverse
join, unnesting that JSONB to answer "which lines currently report this
incident."

Why neither can back the archive:

- **`line_status` is present-tense only** — one row per line, upserted in
  place. It answers "right now", never "in March".
- **`line_status_history` is pruned at 7 days by default**
  (`crates/aggregator/src/config.rs:22-24`,
  `crates/api/src/data/config.rs:196-197`, both `default_value_t = 7`),
  and `prune_history` enforces it (`aggregator/src/queries.rs:487-492`).
  The archive's default view is 30 days and its presets go to "All time"
  (`IncidentSearchForm.tsx:111-121`). A history-backed filter would be
  silently, invisibly truncated at a week.
- It is **snapshot-shaped, not event-shaped**: a row records "these were
  line L's statuses at time T," so reconstructing "incident I affected
  line L at some point" means scanning every snapshot in range and
  unnesting JSONB per row — a very different query from the archive's
  keyset scan of `incidents`.

The archive design already reached this conclusion for catalogue lines
and recorded it as a non-goal
(`2026-09-12-incident-archive-design.md:725-735`): re-deriving historical
match scopes "isn't meaningfully re-derivable for an arbitrary past
incident without also snapshotting all of those inputs as they were at the
time." That reasoning applies with *more* force to custom lines, whose
definitions users edit freely (`update_custom_line`,
`custom_lines.rs:181-216`, mutates operators and stations in place with no
history).

---

## Finding 5 — the privacy boundary, in four parts

### 5a. The plumbing is already session-aware; that is both convenient and dangerous

- `GET /public/incidents` takes **no auth extractor at all** today
  (`routes/incidents.rs:189-192`: `State(app)` + `Query(params)`).
- But the browser path to it *does* carry the session: the same-origin
  proxy forwards the incoming `Cookie` header verbatim
  (`frontend/app/api/[...path]/route.ts:76-79`), and the server-rendered
  path does too — `getAllLines()` uses `cookieForwardInit()`
  (`frontend/lib/api.ts:365-371`, `:91-94`).

A consequence worth stating plainly: **`/incidents`'s page already
receives the caller's own custom lines today.** `getAllLines()` hits
`GET /public/lines`, whose `list_lines` appends
`list_custom_lines_for_user(…, &user.id)` for an authenticated caller
(`crates/api/src/routes/lines.rs:341-380`). `IncidentSearchForm` then
throws them away client-side:

```ts
const catalogueLines = lines.filter((line) => line.source === 'catalogue');
```

(`frontend/components/IncidentSearchForm.tsx:90`). So the caller's own
custom lines are one line of code away from appearing in the dropdown —
and **that is precisely the trap**. Adding them to the `data` array
without a matching backend change produces a form that submits
`?line=custom-my-commute` to a route that answers `400 "unknown line"`.
Adding them *with* a naive backend change is where the real risk starts.

Note also that group-shared lines are **not** in `getAllLines()`'s
output: `list_lines` was deliberately left un-widened by group sharing
(`routes/lines.rs:361-373`, and
`2026-09-12-custom-line-group-sharing-design.md:585-596` — "this list is
'what can I create/edit'… not 'what am I allowed to view'"). The shared
set comes from a different route,
`GET /public/groups/shared-custom-lines`
(`crates/api/src/routes/groups.rs:695-716` →
`crates/api/src/data/groups.rs:1298-1318`), which requires
`AuthenticatedUser` and returns `{groupId, groupName, lineId, lineName,
grantedBy, …}` — ids and names, **not** the line's operators or stations.

### 5b. The oracle risk: never accept a caller-supplied expansion

**The single mistake a future implementer is most likely to make:** have
the frontend expand the chosen custom line into its operator/CRS lists and
send `?operator=SW,GW&line_stations=WOK,SUR,…` (or any equivalent
client-supplied set), leaving the public route unchanged.

That is not merely "no better than today" — it is actively worse than the
current design in two ways:

1. It puts the private line's **definition** on the wire as a plain query
   string, where it lands in access logs, `Referer` headers, browser
   history, and any shareable URL. Today a custom line's station list is
   readable through exactly two gated routes and no others:
   `get_line_definition` (`routes/lines.rs:283-340`,
   `OptionalAuthenticatedUser`, 404 for everyone but the owner and granted
   group members) and `get_line` (`routes/lines.rs:492-555`, which returns
   the full `CustomLineDetail` — `operators`, `stations`,
   `headcode_prefixes`, `destination_crs_filter` — behind
   `AuthenticatedUser` plus the same `readable_custom_line_ids` gate at
   `:522`). Both 404 uniformly; neither leaks the definition to anyone
   else.
2. It makes the *route* accept an arbitrary station set from anyone. An
   attacker who guesses a plausible station list is not learning anything
   from `incidents` (the rows are public) — but the route has now grown a
   filter shaped exactly like "probe me with a private line's contents,"
   and the *next* feature that keys off it inherits that shape.

**The safe shape is the inverse: the client sends only an opaque id; the
server resolves it, and only for a caller who may read it.** Concretely,
a custom-line branch in `routes/incidents.rs` must:

- take `OptionalAuthenticatedUser` (matching `get_line_definition`'s
  posture, `routes/lines.rs:283-288`);
- for a `custom-`-prefixed id, gate on
  `custom_lines::readable_custom_line_ids(pool, &[id], &user.id)`
  (`crates/api/src/data/custom_lines.rs:272-289`) — the single helper
  every other custom-line read path funnels through
  (`routes/lines.rs:321`, `:522`; `routes/line_status.rs:172`, `:373`) —
  **never `owners_for_ids`**, which that function's own doc comment
  (`custom_lines.rs:299-302`) warns would make group-shared lines
  invisible to the members they were shared with;
- keep **one uniform rejection** for all of {no such id, custom id owned
  by someone else, custom id you have no grant for, anonymous caller with
  a custom id, typo'd catalogue id}. Today that is `400 "unknown line"`.
  The rest of the codebase uses `404 "line not found"` and never `403`
  (`routes/lines.rs:307-333`;
  `2026-09-12-custom-line-group-sharing-design.md:559-575` — "a
  non-group-member gets the identical response to a total stranger, by
  construction"). Either is defensible here; **what is not defensible is
  two different responses.** If `custom-alice-commute` 400s with
  "unknown line" while a nonexistent `custom-nope` 400s with a different
  message, or takes a measurably different time, the route has become an
  existence oracle for other users' private line ids — and custom-line ids
  are *slugs of user-chosen names* (`custom_lines.rs:20-36`), i.e.
  guessable (`custom-my-commute`, `custom-work`).

**A neighbouring oracle already exists, and a design should not widen
it.** `insert_custom_line` allocates ids in a **global** namespace,
appending `-2`, `-3`, … on slug collision
(`crates/api/src/data/custom_lines.rs:121-173`, the retry loop at `:161`).
A user who names a line "My Commute" and is handed
`custom-my-commute-4` has just learned that three other lines — almost
certainly other people's — already carry that slug. That is shipped today
and out of scope here, but it is the same risk family, and it is the
reason the guessability argument above is not theoretical: ids in this
namespace are *enumerable by construction*.

One point in favour of feasibility: because every `incidents` row is
already public, a correctly-gated filter leaks **no incident data**. The
entire risk surface is metadata about the *filter value* — whether a given
custom line exists, and what it contains. That is a narrow, closable
surface, which is why "authenticated resolution of an opaque id" is
sufficient rather than needing the route split in two.

### 5c. A live leak, already shipped, in the adjacent route

**`GET /public/incidents/{incidentId}` currently discloses other users'
private custom-line ids and names to anonymous callers.**

`get_incident` (`crates/api/src/routes/incidents.rs:278-297`) has no auth
extractor, and calls
`queries::lines_currently_reporting_incident`
(`crates/api/src/data/queries.rs:1991-2005`):

```sql
SELECT DISTINCT line_status.line_id, line_status.name
FROM line_status, jsonb_array_elements(statuses) AS s
WHERE s -> 'disruption' ->> 'source' = $1
ORDER BY line_status.name
```

There is **no `custom-` filter and no ownership/grant check anywhere on
this path**. The result is rendered straight into the public response as
`currentlyAffectsLines` (`routes/incidents.rs:325-328`) and displayed by
`frontend/app/incidents/[id]/page.tsx:134-141`.

`line_status` demonstrably contains custom-line rows — that is the entire
reason `filter_private_custom_rows` exists
(`crates/api/src/routes/line_status.rs:151-177`), stripping
`id.starts_with("custom-")` rows from the *other* readers of the same
table. `lines_currently_reporting_incident` is a reader of that table that
never got the same treatment.

Concretely: if Alice creates a custom line named "Mum's house → work" on
operator `SW`, and the matcher's Tier 3 gives it an `OperatorOnly` status
sourced from incident `X` (surviving the Finding-3 suppression — it
survives whenever no SWR catalogue line keyword-hits that incident), then
**any anonymous visitor to `/incidents/X` sees `custom-mums-house-work` /
"Mum's house → work" in "Currently affects".**

Why it was missed: the incident detail page
(`2026-08-31-incident-detail-page-design.md`) and private custom lines
(`2026-08-31-private-custom-lines-and-tracked-trains-design.md`) were
designed the same day. The detail design mentions custom lines exactly
once, at `:749`, and only as a *scaling* concern ("Worth revisiting only
if `line_status` ever grows by orders of magnitude (e.g. many more custom
lines)") — never as a privacy one. The group-sharing design's §3.2 audit
enumerated the read gates to widen — its own heading names
`get_line`/`get_line_status`/`get_mode_status`/`get_line_status_history`,
with `get_line_definition` added as a fifth bullet
(`2026-09-12-custom-line-group-sharing-design.md:510-596`). That audit
asked "which gates are too narrow now that sharing exists," so a reader of
`line_status` that had **no** gate at all was never in scope to be
widened, and fell through.

Production could not confirm or refute an *instance* today — one cannot
enumerate other users' custom lines without an account — but the
*mechanism* is demonstrably live: sampling incident-detail responses
returns real, non-empty `currentlyAffectsLines` arrays, every entry of
which is a `line_status` id and name emitted to an anonymous caller with
no gate in front of it. The leak is a straightforward read of the code
path, not a hypothesis.

**Blast radius, stated honestly.** A custom line only appears here if it
reaches Tier 3, which requires at least one operator. Operators are
*optional* on a custom line — `create_line` validates only the name (non-empty, and
sluggable to something past the bare `custom-` prefix) and ≥2 stations
(`routes/lines.rs:445-468`), and `CustomLineForm`
gates only on `stations.length < 2` (`CustomLineForm.tsx:102`) — so a
station-only custom line can never surface this way. That bounds the
exposure; it does not close it, and a user has no way to know that
adding an operator to their private line is also what publishes its name.

**This is arguably a higher-priority fix than the feature this document
researches, and it is independent of it.** The fix is three parts — two
code changes plus the header from §5d below — and **all three** are
required; shipping only the first two is the same half-fix §5d warns
about:

1. `get_incident` takes `OptionalAuthenticatedUser`, and the line refs it
   returns are filtered through `readable_custom_line_ids` exactly as
   `filter_private_custom_rows` (`line_status.rs:151-177`) already does
   for the other readers of `line_status`.
2. **`frontend/lib/api.ts`'s `getIncident()` must start forwarding
   cookies.** It currently does not (`frontend/lib/api.ts:584-588` —
   `cache: 'no-store'` and nothing else), unlike `getAllLines()`
   (`:365-371`), which §5a leans on. Without this, gating `get_incident`
   would mean the server-rendered detail page always calls it
   anonymously, so **the owner would lose their own custom line from
   "Currently affects"** — a silent regression that looks like the fix
   working. Adding the forward also makes `/public/incidents/{id}`
   session-dependent, which is what part 3 exists for.
3. **The §5d cache header on `/public/incidents/{id}`.** Parts 1 and 2
   turn a route that returns identical bytes to everyone into one whose
   body depends on who is asking, while it still ships with no
   `Cache-Control` at all — see §5d. Without this part, the leak is
   closed at the application and re-opened at the edge the first time
   anyone caches that path.

### 5d. A latent caching hazard if the route's output becomes session-dependent

Today `GET /public/incidents` returns the same bytes to everyone for the
same query, so caching it is safe. The moment `?line=custom-…` resolves
differently per caller, the response varies by session.

Measured on production (2026-09-16), the response carries:

```
vary: rsc, next-router-state-tree, next-router-prefetch, next-router-segment-prefetch
cf-cache-status: DYNAMIC
server: cloudflare
```

— i.e. **no `Cache-Control` header at all, and no `Cookie` in `Vary`**,
behind Cloudflare. Nothing is being cached today (`DYNAMIC`), and no
`Cache-Control` was found anywhere in `crates/api/src`. But a future edge
rule that starts caching `/api/incidents` JSON would then serve one
logged-in user's custom-line-filtered results to the next visitor. A
design should specify `Cache-Control: private, no-store` (and/or
`Vary: Cookie`) on any session-dependent response from this route, rather
than relying on the current absence of a caching rule.

The same applies to **`GET /public/incidents/{incidentId}`** the moment
§5c's fix lands, since that fix is precisely what makes the detail
response vary by caller. Both routes change character together; a design
that specifies the header for one and not the other has half-fixed it.

---

## Finding 6 — the UX surface as it stands

`IncidentSearchForm` renders a single-select `Line` control
(`frontend/components/IncidentSearchForm.tsx:380-389`) populated from
`catalogueLines` (`:90`), with a description spelling out the
station-overlap approximation (`:383`). Its header comment states the
current contract explicitly (`:64-67`): "`lines` is filtered to catalogue
lines only (`source === 'catalogue'`) before it is ever offered as a
filter option, matching the backend's own scoping (Decision 2): there is
no way to even attempt filtering by a private custom line from this form."

Worth correcting a common assumption while here: the filter is
**catalogue-only, not catalogue-plus-TfL**. `source === 'catalogue'`
excludes the 13 TfL lines too (109 of 122 ids from `GET /public/lines`
survive the filter — confirmed both from `list_lines`' output and by
counting the 109 options the live page actually renders). TfL line status
is written by `crates/poller-tfl` straight into `line_status` and never
lands in `incidents` at all — "At no point does any TfL data touch
`incidents` or `incident_history`"
(`docs/superpowers/specs/2026-09-07-tfl-incident-page-design.md:115`) — so
there would be nothing for a TfL option to match even if it were offered.
A design that adds a third source to this dropdown should say what it does
about the second one.

The page shell (`frontend/app/incidents/page.tsx`) is a Server Component
with `export const revalidate = 0`, fetching `getAllLines()`/`getAllTocs()`
and seeding initial filters from `searchParams` so filtered links are
shareable.

UX options, sketched only:

1. **One `Line` select, grouped.** Mantine `Select` supports grouped
   `data`; render "Catalogue" / "My lines" / "Shared with me" groups, with
   the last two present only when logged in and non-empty. Cheapest, and
   keeps one mental model of "line". Risk: silently mixes two filters with
   *different matching semantics* (see below) behind one control — the
   thing this codebase's own UI copy is otherwise careful to avoid.
2. **A second, separate control** ("Or one of your lines"), mutually
   exclusive with the catalogue select, with its own description
   explaining its (different) semantics. More honest, more screen real
   estate, and makes "logged out ⇒ absent" trivially obvious.
3. **Reverse the entry point entirely:** no new control on `/incidents`;
   instead a "Search the incident archive for this line" link on
   `/lines/{id}` that deep-links to `/incidents?line={id}`. The page
   already reads `line` from `searchParams`, and `/lines/{id}` is already
   a gated, owner-or-granted-member surface — so the affordance only ever
   appears to someone who can read the line. Smallest UI delta; loses
   discoverability from the archive page itself.

Shared lines need a second fetch in every option (`getAllLines()` does not
include them, §5a), and that endpoint returns only `lineId`/`lineName`
(`data/groups.rs:1298-1318`) — sufficient to populate a dropdown, since
the whole point of §5b is that the client never needs the definition.

An anonymous visitor must see the form exactly as it is today — no empty
"My lines" group, no disabled option, no tooltip saying "log in to filter
by your own lines" that reveals nothing but adds a surface. (`useNeedsLogin`
/ `LoginPromptModal`, used by `CustomLineForm.tsx:9-10`, is the
established pattern if a login prompt is wanted anyway.)

---

## The matching-semantics question, and why it has no obvious answer

Given Finding 2 and Finding 3, "incidents affecting custom line X" could
reasonably mean any of:

**(a) Operator overlap.** `operators && X.operators`. Matches what the
matcher actually does for a custom line today (Finding 3), needs zero
schema work, and is the only option that returns non-empty results
against production data. But it is *broad*: a custom line with three
operators returns every incident on any of them — for a two-station
commute on `SW`, that is every South Western incident network-wide. It
also disagrees with the home page, which shows the suppressed,
false-positive-filtered view (`matcher.rs:70-83`), while this would show
the unsuppressed one.

**(b) Station overlap.** `affected_stations && X.stations`. Narrow,
matches the caller's intuition ("incidents at my stations"), matches the
catalogue filter's existing shape — and returns **nothing, ever**, until
someone populates `affected_stations` (Finding 2).

**(c) Operator ∩ station** (`AND` of both). Strictly narrower than (b),
so also always empty.

**(d) Replay the real matcher.** Run `lines_affected_by` — including the
`OperatorOnly` suppression — per candidate incident against the caller's
custom line. Semantically correct and consistent with the tagline, but it
is aggregator-side logic over live inputs (segment registry, current
catalogue definitions, keyword dictionaries) that the `api` crate does not
have, and running it per row breaks the keyset-paginated `LIMIT n+1` scan
`search_incidents` is built around (`queries.rs:2059-2092`). The archive
design already rejected this for catalogue lines
(`2026-09-12-incident-archive-design.md:725-735`).

**(e) Persist the matcher's verdict and join to it.** A new
`incident_line_matches (incident_id, line_id, scope, matched_at)` table
written by the aggregator each cycle. Turns (d) into an index-friendly
`EXISTS` clause, and would fix the catalogue `line` filter at the same
time. Largest change: new migration, new aggregator write path, a
retention policy, and a decision about what happens when a user *edits* a
custom line (do past matches stay, recompute, or go stale?) —
`update_custom_line` (`custom_lines.rs:181-216`) keeps no history, so
"which definition produced this match" is unanswerable after an edit.

Note that only (a) and (e) produce non-empty results today, and only (d)
and (e) agree with what the rest of the app tells the same user about the
same line.

---

## Do this regardless of which approach wins (or even if neither does)

Two items below are **not** part of the feature and do not depend on
choosing between the approaches. Any plan produced from this document
should carry both as line items, or it is not a complete plan:

- **Close the §5c leak.** `get_incident` gains `OptionalAuthenticatedUser`
  and filters its line refs through `readable_custom_line_ids`, **and**
  `frontend/lib/api.ts`'s `getIncident()` starts forwarding cookies
  (`:584-588`), **and** the detail route gets the §5d cache header. All
  three, or the fix is incomplete in one of the two ways §5c spells out.
  This is shipped-and-live today; it is not hypothetical, and it does not
  need this feature to justify fixing.
- **File the dead catalogue `line` filter as a bug.** Finding 2 measured
  it returning zero rows for all 109 catalogue lines, and
  `2026-09-16-tfl-incident-archive-design.md` §1c found the same thing
  independently and is blocked on it. Whether it is fixed
  here (Approach B does so as a side effect) or elsewhere, it should exist
  as a tracked defect rather than living only inside this research
  document.

## Two candidate approaches

### Approach A — thin: resolve a readable custom line to an **operator** filter

The custom-line branch in `routes/incidents.rs` resolves
`?line=custom-…` via `readable_custom_line_ids` and then binds the line's
`operators` into `search_incidents`'s existing `$1` (`operators && $1`),
**not** its stations into `$2`.

- **Cost:** one route change (an `OptionalAuthenticatedUser` extractor and
  a `custom-` branch), plus frontend dropdown work and a second fetch for
  shared lines, **plus the two "do this regardless" items above** (the
  §5c leak and the §5d cache headers — A touches `routes/incidents.rs`
  anyway, so fixing the leak in the same pass is nearly free). No
  migration, no aggregator change, no new query.
- **Honest:** returns real, non-empty results.
- **Costs to accept:** (i) it is *broad* — the UI must say "incidents on
  any operator this line uses", not "incidents on this line", and must not
  reuse the catalogue filter's station-overlap wording; (ii) it silently
  makes `Line` mean two different things depending on which kind of line
  is selected, unless the UI separates them (UX option 2 or 3); (iii) it
  does not match the suppressed view the home page shows; (iv) it leaves
  the catalogue `line` filter broken and now *visibly* inconsistent — the
  custom filter returns rows while the catalogue one never does.
- **Tempting shortcut to refuse:** because a custom line's operators are
  the only thing being used, someone will observe that the frontend
  already has them and could just set `?operator=…` with no backend change
  at all. That is exactly §5b's mistake. Refuse it.

### Approach B — thorough: persist per-incident line attribution, then join

Add `incident_line_matches`, written by the aggregator alongside
`write_line_status` (it already computes exactly this, per line and per
incident, every cycle — `aggregation.rs` Layer 1). `search_incidents`
grows one optional `line_id` predicate (`EXISTS (SELECT 1 FROM
incident_line_matches m WHERE m.incident_id = incidents.incident_id AND
m.line_id = $n)`), and `routes/incidents.rs` gates a `custom-` id through
`readable_custom_line_ids` exactly as in A.

- **Cost:** migration, aggregator write path + retention/pruning, index
  design, edited-custom-line semantics, **and the two "do this regardless"
  items above** — the §5c leak is not fixed by B, and B adds a *second*
  table carrying `custom-` ids that needs the very same gate.
- **A harder backfill problem than it first looks.** The table could only
  ever be populated forward, and only for incidents that are *live at the
  time*: `queries::load_incidents`
  (`crates/aggregator/src/queries.rs:31-38`) selects `WHERE NOT
  is_cleared`, and `is_active` (`crates/aggregator/src/aggregation.rs:208-220`)
  further drops incidents whose validity window has elapsed or which have
  aged past the next rail-day boundary. So an incident that is cleared
  before the table ships never gets a row, and there is no way to
  retro-fit one from the aggregator — doing so means re-running the
  matcher over historical text, which is option (d), which this document
  and the archive design both reject. For scale: 115 of the 200 most
  recent production incidents are already `isCleared`. A design must
  decide whether an archive filter that silently covers only
  post-deployment incidents is acceptable, and say so in the UI if it is.
- **Wins:** one filter with one meaning for catalogue, TfL and custom
  lines; agrees with the home page and the line detail page because it *is*
  the same verdict; **fixes the catalogue `line` filter as a side effect**;
  `scope` becomes available for a future "precise matches only" toggle.
- **Risks:** the join table is written per-cycle and per-line; a growth
  estimate (incidents × matching lines × retention) is needed before
  committing. Deleting a custom line must cascade (compare
  `delete_custom_line`'s existing transactional `pinned_lines` cleanup,
  `custom_lines.rs:226-242`). And a `custom-` row in that table is itself
  private data, so **every** reader of it needs the same gate — the exact
  failure mode §5c documents.

**These are not ranked here, and the evidence genuinely does not rank
them.** A is right if the goal is "let a user narrow the archive to their
own commute, soon". B is right if the goal is "make `Line` mean one true
thing across the app", and it happens to repair a filter that is currently
broken. The deciding input is one the repo owner holds, not the code: how
much does it matter that the archive's line filter agrees with the rest of
the app?

---

## Open questions a design would have to resolve

0. **Not actually a question — a prerequisite.** The §5c leak
   (`get_incident` → `lines_currently_reporting_incident`, ungated) is
   live today and must be closed whether or not this feature is built; see
   "Do this regardless" above for the three-part fix. It is listed here
   only so that a plan written from this checklist cannot omit it.
1. **Is the broken catalogue `line` filter in scope?** (Finding 2.) If
   yes, Approach B is close to forced. If no, the design must say what
   the UI does about two adjacent filters with wildly different hit rates.
   Either way this finding should probably become its own bug report
   rather than being buried in a feature design — and note that
   `2026-09-16-tfl-incident-archive-design.md` §1c now also blocks on it
   ("until the archive's line-scoped browse works for the railways it
   already covers, extending its coverage … is optimising the wrong end of
   the feature"), so two separate pieces of work are already waiting on the
   same fix.
2. **Should `affected_stations` ever be populated?** The Knowledgebase
   feed has no CRS field, only free-text `RoutesAffected`
   (`common/src/lib.rs:586`) — but `crates/enricher` already runs LLM
   extraction over incident text
   (`2026-08-20-incident-nlp-extraction-design.md`). Extracting CRS codes
   there would revive Tier 1 for every line and make station-overlap
   semantics real. Entirely out of scope here; it changes which approach
   is correct.
3. **`400` or `404` for an unreadable line id?** The route says `400
   "unknown line"` today; the rest of the custom-line surface says `404
   "line not found"`. Pick one and make it uniform across all five
   rejection cases (§5b), including for anonymous callers.
4. **Broad or suppressed?** Does "incidents affecting my line" include the
   `OperatorOnly` matches the matcher deliberately suppresses
   (`matcher.rs:70-83`)? A "yes" disagrees with the home page; a "no"
   under Approach A returns very little, since a custom line's only
   possible scope *is* `OperatorOnly`.
5. **What does an edited custom line mean for past results?** Definitions
   are mutated in place with no history (`custom_lines.rs:181-216`).
   Under A this is harmless (the filter is always evaluated against the
   current definition). Under B it must be decided explicitly.
6. **Does the archive URL stay shareable?** `/incidents?line=custom-x` is
   a link that works for the owner, works for granted group members, and
   rejects for everyone else. Is a URL that renders differently per
   recipient acceptable on a page whose design deliberately made filtered
   links shareable (`app/incidents/page.tsx`)?
7. **Should `GET /public/incidents` stay in `public_router()`?** It would
   become a route whose behavior depends on a session while living under
   a prefix whose stated convention is "unauthenticated"
   (`routes/incidents.rs:1-7`). `get_line_definition` already sets the
   precedent for `OptionalAuthenticatedUser` under `/public`, so this is
   a naming/consistency question, not a blocker — but it should be
   answered, not drifted into.
8. **Cache headers.** (§5d.) Which exact header, applied to the whole
   route or only to session-dependent responses?
9. **Group-shared lines in the dropdown: whose name?** A shared line's
   `name` is its owner's wording. `list_shared_custom_lines_for_user`
   returns `grantedBy`/`groupName` alongside it
   (`data/groups.rs:1298-1318`) precisely so the UI can attribute it —
   worth doing here too, or two members' lines named "Commute" are
   indistinguishable.

## Explicit non-goals of any follow-up design

- Populating `affected_stations` (Q2) — separate feature, separate spec.
- Retention/pruning for `incidents`/`incident_history`, already flagged as
  an open gap by `2026-09-12-incident-archive-design.md`.
- Any write path on `/public/incidents`.
- Anonymous access to a custom line, in any form. Group sharing was
  explicit that "there is no anonymous access to a shared custom line,
  ever" (`2026-09-12-custom-line-group-sharing-design.md:557-558`); a
  filter must not become the exception.
- Widening `list_lines` to include group-shared lines. It was left
  un-widened on purpose (`routes/lines.rs:361-373`); a filter dropdown
  that needs both sets should make two calls, not blur that distinction.

## References

- `crates/api/src/data/queries.rs:83-220` (`upsert_incidents`, the one
  production writer of `incidents`), `:2069-2136` (`search_incidents`),
  `:2089-2090` (the two array-overlap filters), `:1991-2005`
  (`lines_currently_reporting_incident`)
- `crates/api/src/routes/incidents.rs:52-91` (`IncidentSearchParams`),
  `:189-276` (`search_incidents` handler), `:203-215` (catalogue-line →
  CRS resolution), `:278-297` (`get_incident`), `:325-328`
  (`currentlyAffectsLines` rendering), `:492-898` (`mod db_tests`, whose
  fixtures seed a data shape production never produces)
- `crates/api/src/data/custom_lines.rs:20-36` (`slugify`), `:181-216`
  (`update_custom_line`), `:226-242` (`delete_custom_line`), `:272-289`
  (`readable_custom_line_ids`), `:303-320` (`owners_for_ids`, with its
  "don't use me for new read gates" warning at `:299-302`), `:330-351`
  (`list_custom_lines_for_user`)
- `crates/api/src/routes/lines.rs:283-340` (`get_line_definition`'s gate),
  `:341-380` (`list_lines`' deliberate own-lines-only scoping),
  `:445-468` (`create_line`'s validation — no operator is required),
  `:492-555` (`get_line`, the other gated route that returns a custom
  line's full definition)
- `crates/api/src/routes/line_status.rs:151-177`
  (`filter_private_custom_rows` — the pattern `lines_currently_reporting_incident`
  is missing)
- `crates/api/src/routes/groups.rs:695-716` and
  `crates/api/src/data/groups.rs:1298-1318`
  (`GET /public/groups/shared-custom-lines`)
- `crates/common/src/lib.rs:585-586` (`affected_stations` is "left empty by
  pollers"), `:1156-1174` (`CustomLine`), `:1176-1207`
  (`From<CustomLine> for LineDefinition`)
- `crates/poller-incidents/src/schema.rs:106` (`affected_stations: vec![]`)
- `crates/aggregator/src/matcher.rs:39-86` (`lines_affected_by` and the
  `OperatorOnly` suppression), `:88-185` (`match_one`'s three tiers)
- `crates/aggregator/src/aggregation.rs:36-45` (`merge_custom_lines`,
  doc comment at `:30-35`), `:54-68` (a report is seeded for every line in
  the merged map), `:156` (the `knowledgebase-incident-{id}` source
  string), `:208-220` (`is_active`)
- `crates/aggregator/src/main.rs:193-194` (custom lines merged every
  cycle), `:234-240` (every report written through `write_line_status`)
- `crates/aggregator/src/queries.rs:31-38` (`load_incidents`, `WHERE NOT
  is_cleared`), `:434-483` (`write_line_status`),
  `:487-492` (`prune_history`)
- `crates/aggregator/src/config.rs:22-24` and
  `crates/api/src/data/config.rs:196-197`
  (`history_retention_days`, default 7)
- `frontend/components/IncidentSearchForm.tsx:64-67` (the current stated
  contract), `:90` (`catalogueLines` filter), `:380-389` (the `Line`
  select and its approximation copy)
- `frontend/app/incidents/page.tsx` (server shell, `revalidate = 0`,
  `searchParams` seeding)
- `frontend/app/incidents/[id]/page.tsx:134-141` (renders
  `currentlyAffectsLines`)
- `frontend/app/lines/CustomLineForm.tsx:95-139` (submit),
  `:144-223` (the four fields a custom line actually has)
- `frontend/lib/api.ts:91-94` (`cookieForwardInit`), `:365-371`
  (`getAllLines`, which uses it), `:584-588` (`getIncident`, which does
  **not** — the gap §5c's fix has to close)
- `frontend/app/api/[...path]/route.ts:76-79` (cookie forwarding through
  the same-origin proxy)
- `frontend/app/layout.tsx:50` ("an incident is only ever flagged on the
  lines it actually affects")
- `docs/superpowers/specs/2026-09-12-incident-archive-design.md:160-179`
  (public-read convention and the custom-line oracle note), `:203-250`
  (Decision 2), `:725-745` (non-goals, including the historical-matching
  and custom-line ones)
- `docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md:510-596`
  (§3.2 — `readable_custom_line_ids`, the read gates it backs, and the
  deliberate `list_lines` exclusion)
- `docs/superpowers/specs/2026-08-31-incident-detail-page-design.md:744-749`
  (the only mention of custom lines — a scaling note, not a privacy one)
- `docs/superpowers/specs/2026-09-05-incident-line-matching-false-positive-design.md`
  (the `OperatorOnly` suppression rule and its rationale)
- `docs/superpowers/specs/2026-09-16-tfl-incident-archive-design.md:163-216`
  (§1c — an independent, same-day confirmation of Finding 2 from a
  different brief, with its own live probes)
- `docs/superpowers/specs/2026-09-07-tfl-incident-page-design.md:115`,
  `:131-152` (TfL data never reaches `incidents`; the full
  poller → `upsert_incidents` → aggregator → public-route path)
- Production probe of `https://ds.cursed.solutions/`, 2026-09-16:
  `GET /api/incidents?limit=200` paged to exhaustion (8 pages, 1507
  incidents, 2026-09-03 → 2026-09-16) — 1507/1507 with operators, 0/1507
  with `affectedStations`;
  `GET /api/incidents/815A2D9F55D3477982B19529C65E545D`;
  `GET /api/lines` anonymous (109 catalogue + 13 TfL, 0 custom — the
  `list_lines` privacy filter confirmed working);
  `GET /api/incidents?limit=1&line={id}` for **all 109** catalogue line
  ids, all time — 0/109 returned any row, each a `200` with
  `{"nextCursor":null,"results":[]}`; response headers showing no
  `Cache-Control` and `cf-cache-status: DYNAMIC`
- Headless-Chromium UI check of `https://ds.cursed.solutions/incidents`,
  anonymous, 2026-09-16: the `Line (optional)` select lists 109 options
  (catalogue only — no TfL, no custom), and selecting one produces "No
  incidents match these filters."
