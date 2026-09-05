# Country Filtering (GB / Northern Ireland / Republic of Ireland) — Design Spec

**Status: design spec, not an approved plan.** No implementation, no code
in this pass.

## What was asked for

`docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md` ("the
Ireland spec") — merged to `main`, authoritative, not re-litigated here —
proposes adding Northern Ireland (NIR/Translink) and Republic of Ireland
(Iarnród Éireann) rail data to this app via a new generic
`common::IslandOfIrelandStation`/`IslandOfIrelandLineDefinition` type
tagged with `IslandOfIrelandNetwork::{NorthernIreland, RepublicOfIreland}`
(Ireland spec §3), and resolves the Belfast–Dublin Enterprise cross-border
overlap by sourcing it entirely from Iarnród Éireann (§4). Its own
Non-goals explicitly deferred all frontend/UI design to implementation
time. This document is that follow-up for one specific piece of it: once
Ireland (NI + ROI) rail data exists alongside this app's existing GB
National Rail + TfL London-transport data, should users be able to filter
this app's list views by country/jurisdiction — GB vs. Irish/NI — not
just by the existing "mode" filtering, and if so, how.

A separate implementation plan for the Ireland Tier A/B backend work
(`docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md`) does
**not exist yet** — checked at the start of this pass; the
`worktree-ireland-plan` branch that will presumably hold it has no commits
past `origin/main` yet. This document proceeds without it, per the brief,
but §8 (Open Questions) flags exactly where its eventual data-model
choices could force a revision here.

---

## 1. How filtering actually works today — precisely, since "country" must extend or coexist with it

This section corrects an assumption implicit in "extend the existing
mode-filtering mechanism": **there is no existing user-facing mode
filter to extend.** Two genuinely different mechanisms both get loosely
called "filtering" in this codebase, and they matter differently here.

### 1.1 `mode` is a fixed server-side fetch scope, not a user-toggleable filter

`frontend/lib/modes.ts:12-19` (`DISPLAYED_MODES`) and `:24`
(`DISPLAYED_MODES_PARAM`) are a **hardcoded constant** — "every mode this
app ever displays" — interpolated into `GET /Line/Mode/{modes}/Status`
by both list pages (`frontend/app/lines/page.tsx:26-28`,
`frontend/app/page.tsx:121-122`). No UI lets a visitor choose a subset;
every page render fetches the same fixed six-mode set every time.
Server-side, `crates/api/src/routes/line_status.rs:168-175`
(`SUPPORTED_MODES`) is a closed allow-list `parse_modes`
(`:181-199`) validates the `{modes}` path segment against, returning 400
for anything not in the six — a typo guard and an anti-"real TfL mode we
deliberately don't ingest" guard (`bus`, `river-bus`, `cable-car`), not a
user-permission mechanism. There is nothing here shaped like a filter
control; extending it would mean building one from scratch, not folding
a new dimension into an existing one.

### 1.2 The operator `MultiSelect` in `AllLinesTable.tsx` is the real precedent

`frontend/app/lines/AllLinesTable.tsx` has the only actual user-facing
list filter in this app today: "Filter by operator"
(`:169-178`), a Mantine `MultiSelect` whose options are derived from
the already-fetched data itself (`operatorOptions`, `:88-97`,
`Array.from(new Set(lines.flatMap(...)))`), held in local component
state (`selectedOperators`, `useState`, `:76`), applied client-side over
rows the page already fetched (`filteredRows`, `:117-124`), with no URL
persistence at all — refresh the page and the filter resets. It also
has one non-trivial matching wrinkle worth carrying forward as a
pattern, not the specific logic: `expandOperatorForFiltering`
(`:52-63`) widens a single selected value ("TfL") into every code that
should count as a match ("TfL", "LO", "XR") without changing what the
option list itself shows or what a row's own badge displays — selection
semantics and displayed data are deliberately kept separate.

**This — not `mode` — is the shape a country filter should copy**: a
self-populating `MultiSelect` (or equivalent) over already-fetched rows,
local `useState`, no new URL param, no new API round trip.

### 1.3 Station search is a separate surface with no filter of any kind today

`frontend/app/stations/StationSearchForm.tsx` is a single `Autocomplete`
with no filter control at all — it calls `searchStations`
(`frontend/lib/suggestions.ts:9-14`), which hits
`GET /public/stations?q=...` (`crates/api/src/routes/reference.rs:18-42`),
which runs `reference::search_stations`
(`crates/api/src/data/reference.rs:53-75`) — a query directly against a
`stations` **database table** keyed by `crs`/`name` columns
(`SELECT crs AS code, name FROM stations WHERE crs ILIKE $1 OR name ILIKE
$1 ...`). This is a different store entirely from the TOML-authored
catalogue's `common::LineDefinition.stations: Vec<Station>`
(`crates/common/src/lib.rs:461-500`, `:443-451`) — it's populated by a
poller, not read from `lines/*.toml`. Two consequences for this
document: (a) whatever ends up backing Ireland station search will not
automatically inherit anything this document designs for the `stations`
table, and (b) per the Ireland spec's own Non-goals (§3 closing
paragraph, restated in this task's brief), the CRS-keyed `/stations/[crs]`
route and this search flow do not fit non-CRS Irish/NI stations at all —
a new, separate route/search flow is implementation-time work the Ireland
spec explicitly deferred, and this document does not design it either
(see §7, Non-goals).

### 1.4 Neither `Station` nor `LineDefinition` carries any jurisdiction concept today

`crates/common/src/lib.rs:443-451` (`Station`) and `:461-500`
(`LineDefinition`) have no country/jurisdiction field. Every line and
station in the existing catalogue is implicitly Great Britain by
convention (it is the entirety of what this app has ever ingested), not
by an explicit tag anywhere in the type or the data.

---

## 2. Decision 1 — the country value set

**GB, Northern Ireland, Republic of Ireland — three values, mirroring
`IslandOfIrelandNetwork` exactly, with GB added as the implicit "not
Ireland" default rather than as a fourth explicit enum variant anywhere
in `common::`.**

- `NorthernIreland` and `RepublicOfIreland` reuse the Ireland spec's own
  `IslandOfIrelandNetwork` (§3) verbatim — there is no reason to invent a
  different taxonomy for the same two jurisdictions the data model
  already names, and doing so would create two competing vocabularies
  for an identical distinction.
- **No separate value for TfL/London-transport vs. National Rail within
  GB.** The brief asks this explicitly, and the answer is no: `mode`
  already carries that distinction (`national-rail` vs. `tube`/`dlr`/
  `overground`/`elizabeth-line`/`tram`) and does so orthogonally to
  jurisdiction — every one of those six modes is GB-only today, and
  splitting "GB" into "GB National Rail" / "GB TfL" as country values
  would duplicate information `mode` already expresses precisely, for no
  new distinguishing power. Country answers "which jurisdiction," mode
  answers "which network within it" — collapsing them back into one
  dimension would make a future GB-only, cross-mode view (e.g. "hide all
  Irish/NI content") require selecting multiple values instead of one.
- **The Enterprise/border-area resolution (Ireland spec §4) means
  `network`/country is "which feed is authoritative for this row," not
  literal geography** — Belfast, Lisburn, Portadown, Lurgan, and Newry
  are physically in Northern Ireland but are tagged
  `RepublicOfIreland` because Iarnród Éireann is their sourcing feed.
  Country filtering inherits this framing unchanged: filtering to
  "Northern Ireland" will not surface those five stations/the Enterprise
  line, and filtering to "Republic of Ireland" will. This is a direct,
  unavoidable consequence of a decision already made and cited above
  (§4 there), not a new inconsistency this document introduces — but it
  is worth stating plainly since a user unfamiliar with §4 could
  reasonably expect "Northern Ireland" to mean "physically in Northern
  Ireland."

---

## 3. Decision 2 — GB gets no explicit tag anywhere in `common::`; "not in the Ireland catalogue" is GB by construction

The brief asks this directly: does GB become a first-class value
requiring a migration/default value at every existing `LineDefinition`/
`Station` call site, or does "country filtering" only ever need to
distinguish "the existing GB+TfL catalogue" (one opaque bucket) from "the
Ireland catalogue" (subdivided per `IslandOfIrelandNetwork`)?

**Decision: the latter. Do not add a `country`/`jurisdiction` field to
`common::Station` or `common::LineDefinition`. GB is never an explicit
tag anywhere — it is simply "belongs to the pre-existing GB catalogue,"
inferred by which type/table a row lives in, never by a field on it.**

Reasoning, weighed directly:

- **Cost is wildly asymmetric.** `Station`/`LineDefinition` are read and
  constructed at dozens of call sites across `crates/aggregator`,
  `crates/api`, `crates/poller-tfl`, every `lines/*.toml` catalogue file,
  and this file's own test fixtures (`test_catalogue_line`, repeated
  near-verbatim in at least `crates/api/src/routes/lines.rs:847-877` and
  `:1294-1324`, and `crates/api/src/routes/line_status.rs:1294-1324`).
  Adding a mandatory new field — even one with a `#[serde(default)]` —
  touches every one of those construction sites for a value that is
  100% constant (`Gb`) across every one of them today, and stays that way
  until an entirely separate ingestion pipeline (a future GB-catalogue
  change, not this document's concern) ever needs it to vary. That is a
  large, wide-blast-radius change purchasing zero new distinguishing
  power over the alternative.
- **The Ireland spec's own `IslandOfIrelandStation`/`LineDefinition` are
  already a separate type** (§3 there), deliberately not folded into
  `common::Station`/`LineDefinition` (the same document's own reasoning:
  `Station.crs` is required and non-`Option`, with no NIR/Iarnród
  Éireann-shaped value). That separation is the load-bearing fact this
  decision leans on: **the type itself is already the country tag for
  the GB/non-GB split.** Anything constructed as `common::Station`/
  `LineDefinition` is GB by construction; anything constructed as
  `IslandOfIrelandStation`/`LineDefinition` already carries an explicit
  `network: IslandOfIrelandNetwork` for the NI/ROI split. No third field
  anywhere is needed to recover "which of the three" for any given row —
  it's answerable from (type, and if Ireland, `.network`) alone.
- **Symmetry with `LineStatus`/`LineStatusReport` holds the same way.**
  These are keyed by `mode_name` (`LineStatusReport.mode_name`,
  `crates/common/src/lib.rs` — see `to_report`,
  `crates/api/src/routes/line_status.rs:81-89`), not by any station/line
  struct at all, once a poller has already computed a status row. Country
  for *that* surface is a lookup over `mode_name`, not a field that
  needs to exist on `Station`/`LineDefinition` — see Decision 3 below.
- **Against, considered and rejected:** an explicit `Gb` variant would
  make every future country-aware query pattern uniform (`match
  country { Gb => .., NorthernIreland => .., RepublicOfIreland => .. }`)
  rather than "GB is absence of the other two." This is a real, if minor,
  ergonomic loss — but it is not worth the migration cost above, and
  nothing in this app's current codebase treats "GB" as needing to be
  pattern-matched symmetrically with the other two; it only ever needs
  to be *excluded* or *included* as a bucket, which "not tagged Ireland"
  already answers correctly and cheaply.

---

## 4. Decision 3 — line-status list surface: derive country from `mode_name`, no new API surface, filter client-side

For `AllLinesTable.tsx` (the `/lines` page) and any future page built the
same way:

**Decision: country is derived client-side from each row's `mode_name` /
`category` via a small static lookup table (`MODE_TO_COUNTRY`, shaped
like `frontend/lib/modes.ts`'s existing constants), not sent as a new
query parameter and not added as a new field to any API response body.**

- Every mode this app can ever emit a `LineStatusReport` for already
  maps to exactly one country: today's six (`national-rail`, `tube`,
  `dlr`, `overground`, `elizabeth-line`, `tram`) all map to `Gb`. Once
  (if) NI/ROI line-status pollers exist, whatever `modeName` values they
  emit — per the Ireland spec's own closing paragraph in §3 ("a new
  `modeName` value per network, e.g. `nir-railways`,
  `iarnrod-eireann`/`irish-rail`") — map one-to-one to `NorthernIreland`/
  `RepublicOfIreland`. A lookup table over an already-present field is
  strictly cheaper than a new column threaded through
  `line_status`/`line_status_history`, a new field on every JSON body
  `to_tfl_shape`/`to_tfl_shape_with_overlay` produce
  (`crates/api/src/render.rs:14-44`), and a new thing for every existing
  API consumer (including anything mimicking TfL's own API shape, which
  this app's four `/Line/...` routes deliberately match — see
  `crates/api/src/routes/line_status.rs:1-10`) to ignore or break on.
- **This mirrors `MERGED_TFL_LINE_IDS`/`TFL_TO_NR_LINE_ID`'s existing
  precedent exactly** (`frontend/lib/modes.ts:35-43`,
  `crates/common/src/lib.rs`) — a small, hand-maintained frontend/shared
  mapping table over an id/mode this app already emits, not a new
  backend column, for a fact ("which line is this really the same
  service as") that's likewise derivable from what already exists.
  Country-from-mode is the same shape of derivation.
- **UI**: a `MultiSelect` (or, given only 2-3 values are ever possible,
  a segmented control/`Chip.Group` may read better than a searchable
  multi-select built for dozens of operator codes — a UI-affordance
  choice left to implementation, not decided here) built exactly like
  `operatorOptions` (`AllLinesTable.tsx:88-97`): computed from the
  countries actually present in the fetched `lines`/`reports` for this
  render, held in local `useState`, applied client-side alongside
  (AND-combined with, matching how a real user would expect two
  independent filters to compose) the existing operator filter — not
  folded into it, since operator and country answer different
  questions and a line can only ever have one country but potentially
  several operators.
- **No URL persistence**, matching the operator filter's own precedent
  (§1.2) rather than inventing new URL-param machinery neither existing
  filter has.

**The one large caveat this decision depends on, stated plainly**: this
entire surface currently has **nothing to filter**, and may not for a
long time. Per the Ireland spec §5/§7, Tier C (line-status/incident
parity — the thing that populates `line_status` rows with a
`LineStatusReport` at all) is a **no-go for both NI and ROI**, unchanged
by that document's own final recommendation. Until some future Tier C
work ships for at least one Irish network, `AllLinesTable`'s `lines`
prop (from `GET /public/lines`,
`crates/api/src/routes/lines.rs:132-190`) will contain zero non-GB rows
even if Tier A/B static and live-departure data exists elsewhere in the
app, because `list_lines` only ever reads the GB catalogue
(`app.config.lines`), TfL-sourced rows (`queries::tfl_line_summaries`),
and custom lines — none of which is where Ireland Tier A/B data would
live per the Ireland spec's own scope. **A country filter control on
this page is therefore currently speculative UI for data that does not
exist and is not currently planned to exist** (see §8, Open Question 1).

---

## 5. Decision 4 — self-hiding gate, not a feature flag: the control appears only when the data justifies it

The brief asks whether a country filter should be user-visible
immediately (before Ireland data ships) or gated behind Ireland data
actually landing.

**Decision: gated, but not via an explicit feature flag — the same
"derive options from what's actually in the data" pattern already used
for `operatorOptions` naturally self-hides the control until it has more
than one distinct value to offer.**

Concretely: compute the set of countries present in the current render's
`lines`/`reports` exactly as `operatorOptions` computes its set from
`lines.flatMap((line) => line.operators)`
(`AllLinesTable.tsx:88-97`); if that set has fewer than two members
(today, always exactly `{Gb}`), don't render the filter control at all —
there is nothing meaningful to filter by, and a one-option "Filter by
country: GB" dropdown is worse than no control (dead UI, an implicit
false promise that other values might appear if you look). The moment a
second country's rows appear in the underlying data (whenever Tier C, or
whatever surface actually populates them, ships), the control appears
automatically, with zero additional code path to remember to flip on.
This is strictly better than a manual feature flag: no separate rollout
step, no risk of the flag being forgotten and the flag itself becoming
stale scaffolding (a failure mode this repo has hit before — see e.g.
the Decision-4 scaffolding routes in `line_status.rs:59-64` that
`return []` until a real producer exists, which is the same
"pre-build the shape, gate on data" idea already in this codebase).

---

## 6. The station-search surface: not designed here, deliberately

Per the brief and per this document's own Non-goals (§7), the Ireland
station-page/search-flow UI is a separate, later, implementation-time
piece — the Ireland spec's own §3 closing paragraph and Non-goals
already deferred it, and duplicating or preempting that design here
would create two competing proposals for the same not-yet-built surface.

What this document does commit to, since it's a real interaction the
Ireland UI work will need to know about: **if/when Ireland station
search exists as its own surface, whatever country-filtering vocabulary
it uses should be the same three values as §2 above (GB/`NorthernIreland`/
`RepublicOfIreland`), not a separately invented taxonomy** — the same
reasoning as Decision 1's rejection of a competing vocabulary applies
here too. Beyond naming reuse, this document takes no position on: what
that search surface's underlying query looks like, whether it's a single
combined GB+NI+ROI search or is deliberately split by jurisdiction (a
question the Ireland spec's own open §3 framing — "a shared non-CRS
station route... or two" — already flagged as unresolved), or what its
URL/route shape is.

---

## 7. Non-goals

- **Designing Ireland's station-page routes or search-flow UI**
  (`/ni-stations/[id]`-shaped vs. a shared route, or any query/filter
  mechanism specific to that surface). Already deferred by the Ireland
  spec (§3) to a separate, later piece of implementation-time work; not
  duplicated or preempted here beyond the one vocabulary-reuse note in
  §6.
- **Any backend schema change.** No migration, no new column on
  `line_status`/`line_status_history`/`stations`, no new field on
  `common::Station`/`LineDefinition`/`IslandOfIrelandStation`/
  `IslandOfIrelandLineDefinition` beyond what the Ireland spec already
  specified (`network: IslandOfIrelandNetwork`).
- **A new query parameter on any existing `api` route.** Decision 3
  concludes this is unnecessary for the line-status list surface, and §6
  explicitly declines to design the station-search surface where a
  parameter might eventually matter.
- **Deciding whether/when Ireland Tier C (line-status parity) ever
  ships.** That is the Ireland spec's own go/no-go call (§7 there,
  currently no-go for both networks); this document only reasons about
  the consequence for country filtering's applicability (§4's caveat),
  it does not revisit or lobby for that decision.
- **The exact visual form of the filter control** (`MultiSelect` vs.
  `Chip.Group`/segmented control) — flagged in §4 as an
  implementation-time choice between reasonable options, not decided
  here.
- **An implementation plan.** A separate, later step, matching this
  repo's own process.

---

## 8. Open Questions

1. **The single biggest risk: this design's primary surface (the
   `/lines` list) may have nothing to filter for a long time, or ever,
   depending on a decision this document doesn't control.** Country
   filtering on `AllLinesTable` is only meaningful once at least one
   non-GB `LineStatusReport`-producing pipeline exists (Ireland spec
   Tier C, currently no-go for both NI and ROI — §5/§7 there). If that
   never changes, everything in §4 is correct but permanently inert, and
   the only surface where country filtering would ever have real data to
   act on is station search — which this document explicitly declines to
   design (§6/§7). A future reader should treat §4/§5 as "ready the day
   it's needed," not "needed soon."
2. **Depends directly on the not-yet-written Ireland implementation
   plan**: Decision 3's `MODE_TO_COUNTRY` lookup table cannot actually be
   written until whatever poller(s) the Ireland plan builds settle on
   real `modeName` string values for NI/ROI line-status rows (the
   Ireland spec's own `nir-railways`/`iarnrod-eireann`/`irish-rail`
   examples in §3 are illustrative, not committed). If the eventual plan
   instead reuses `national-rail` as the `modeName` for Iarnród
   Éireann-sourced Enterprise-corridor rows (plausible, since §4 there
   frames Iarnród Éireann sourcing as "authoritative feed," and its data
   already shares a lot of shape with GB LDBWS-style inference), the
   clean one-mode-to-one-country mapping this document assumes would
   break for at least that one line, and Decision 3 would need a
   per-line-id override table (mirroring `severity_overrides`'s existing
   per-line-TOML-field precedent) rather than a pure `modeName` lookup.
   Flagged here rather than guessed at, per this repo's own citation
   discipline.
3. **Whether `IslandOfIrelandStation`/`LineDefinition` rows (Tier A/B)
   ever get exposed through `GET /public/lines` or an equivalent listing
   endpoint at all**, given §4's finding that `list_lines`
   (`crates/api/src/routes/lines.rs:132-190`) only reads
   `app.config.lines` (the GB TOML catalogue), TfL rows, and custom
   lines today. If Ireland Tier A/B data ends up surfaced through an
   entirely different endpoint shape (plausible, since it isn't a
   `LineDefinition` at all), this document's country-filter-on-`/lines`
   design may need a second, parallel treatment for wherever that data
   actually appears — not knowable until that surface exists.
4. **UI affordance choice (`MultiSelect` vs. segmented control/`Chip.Group`)
   for a 2-3-value filter** — flagged as a Non-goal (§7), left to
   whoever implements this once real data exists to inform the choice.

---

## References

- `docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md` —
  authoritative source for `IslandOfIrelandNetwork`/
  `IslandOfIrelandStation`/`IslandOfIrelandLineDefinition` (§3), the
  Enterprise border-overlap sourcing resolution (§4), and the Tier A/B/C
  go/no-go recommendation this document's §4/§8 depend on (§5/§7 there).
- `frontend/lib/modes.ts:12-19,24,35-43` — `DISPLAYED_MODES`/
  `DISPLAYED_MODES_PARAM`/`MERGED_TFL_LINE_IDS`, cited in §1.1 and as the
  structural precedent for Decision 3's `MODE_TO_COUNTRY` table.
- `crates/api/src/routes/line_status.rs:168-175,181-199,81-89` —
  `SUPPORTED_MODES`, `parse_modes`, `to_report`; cited in §1.1 and §3.
- `frontend/app/lines/AllLinesTable.tsx:52-63,76,88-97,117-124,169-178` —
  the operator `MultiSelect`, cited throughout §1.2/§4/§5 as the
  structural precedent this document's recommended country filter
  copies.
- `frontend/app/stations/StationSearchForm.tsx`,
  `frontend/lib/suggestions.ts:9-14`,
  `crates/api/src/routes/reference.rs:18-42`,
  `crates/api/src/data/reference.rs:53-75` — the station-search
  surface's full round trip, cited in §1.3/§6 as the surface this
  document declines to design filtering for.
- `crates/common/src/lib.rs:443-451,461-500` — `Station`/
  `LineDefinition`, cited in §1.4/§3 as having no jurisdiction field
  today and as the basis for Decision 2's "type is the tag" reasoning.
- `crates/api/src/routes/lines.rs:132-190` — `list_lines`, cited in §4/§8
  as the endpoint `AllLinesTable`'s `lines` prop comes from and the
  reason it currently cannot contain any Ireland row regardless of Tier
  A/B status.
- `crates/api/src/routes/line_status.rs:59-64` — the Decision-4
  full-coverage scaffolding routes, cited in §5 as an existing precedent
  for "build the shape, gate on data arriving" over a manual feature
  flag.
