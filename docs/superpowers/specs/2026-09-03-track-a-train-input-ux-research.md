# Track a Train — Input UX Research

**Status: research/scoping only, not an approved design.** Written to the same
rigor and shape as `docs/superpowers/specs/2026-09-03-per-station-stats-research.md`
and `docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`
(citation-heavy audits of an existing feature's real shape, ending in a
recommendation plus an explicit open-questions list where the picture isn't
fully decidable, not a committed implementation plan). This document does not
propose an implementation plan — per this repo's process, that would be a
separate, later step once a direction here is actually picked.

Two related but very differently-sized questions about `TrackTrainForm`'s
inputs, kept in one document because they're two points on the same spectrum
("how much does the user have to type by hand to track a train"), and Part 2's
scoping conclusion depends directly on Part 1 already being cheap:

- **Part 1**: give the existing Origin/Destination CRS and Operator text
  fields autocomplete, reusing infrastructure this codebase already has and
  already uses twice elsewhere.
- **Part 2**: whether the schedule feed pipeline fix unlocks something bigger
  — real trip/service search instead of typing raw codes and a time at all.

## Part 1: CRS code and operator field autocomplete

### The current form, exactly as it stands

`frontend/components/TrackTrainForm.tsx:137-185` renders four fields, all
plain, no autocomplete:

- `TextInput` "Origin CRS code" (`TrackTrainForm.tsx:137-144`), required,
  validated client-side only by `CRS_PATTERN = /^[A-Za-z]{3}$/`
  (`TrackTrainForm.tsx:12,60`) — a three-letter regex, nothing checked
  against the real station reference set.
- `DateTimePicker` "Scheduled departure" (`TrackTrainForm.tsx:146-158`),
  required — untouched by this document; DateTimePicker is not a
  text-autocomplete target.
- `TextInput` "Destination CRS code (optional)" (`TrackTrainForm.tsx:174-179`)
  — optional, no validation at all client-side (any string is accepted;
  server-side validation is discussed below).
- `TextInput` "Operator (optional)" (`TrackTrainForm.tsx:180-185`) — optional,
  free text, no validation.

On submit (`TrackTrainForm.tsx:84-90`), all three are folded into
`common::TrackPinRequest` (`crates/common/src/lib.rs:557-565`) as plain
strings: `origin_crs` and `scheduled_departure` required,
`destination_crs`/`operator` sent only `if trim()` is non-empty (`Option<String>`
on the Rust side, `#[serde(skip_serializing_if = "Option::is_none")]`).
Server-side, `crates/api/src/data/train_tracking.rs:47-69`'s `validate_pin`
checks only that `origin_crs` is non-empty and exactly 3 characters, plus the
`MAX_PIN_AGE` departure-time check — it does not check `destination_crs` or
`operator` against anything at all, and does not check `origin_crs` against
the real station reference set either (it would accept `"XXX"`).

This gap is not accidental oversight — it was raised and explicitly deferred
at design time. `docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md:611-617`,
Open Question 5: *"Whether `/track`'s manual-entry origin CRS should validate
against the real station reference set (`getStationName`/`/public/stations`
type-ahead, already used elsewhere) rather than just a 3-letter regex... left
as a planning-time detail."* That question was never picked back up in a
later design or implementation — `TrackTrainForm.tsx` as shipped still only
does the bare regex.

### The template: how `CustomLineForm.tsx` wires this up today

Two working examples exist in this exact codebase, both built on the same
two pieces of shared infrastructure:

- `frontend/lib/useSuggestions.ts:13-55` — a hook that debounces a query
  string by 250ms, calls a supplied `search(query, signal)` function, and
  aborts the in-flight request if the query changes again before it resolves
  (comment at `useSuggestions.ts:8-12`: *"Shared by every operator/station
  autocomplete field."*).
- `frontend/lib/suggestions.ts:9-21` — `searchStations`/`searchTocs`, both
  thin client-side fetches against the same-origin proxy (`GET /api/stations?q=`,
  `GET /api/tocs?q=`), short-circuiting on an empty query without a network
  call. These proxy through to `crates/api/src/routes/reference.rs`'s
  `/stations`/`/tocs` — unauthenticated, read-only type-ahead endpoints
  (module doc, `reference.rs:1-4`) capped at 20 results per query
  (`SUGGESTION_LIMIT`, `reference.rs:16`) and backed by
  `crates/api/src/data/reference.rs`'s `search_stations`/`search_tocs`. Both
  return `Suggestion { code, name }` (`frontend/lib/types.ts:225-228`).

`frontend/app/stations/StationSearchForm.tsx:9-13` is the simplest real
usage: a Mantine `Autocomplete` fed by `useSuggestions(crs, searchStations)`,
`data={suggestions.map(s => ({ value: s.code, label: s.code }))}` with
`filter={({ options }) => options}` disabling Mantine's own client-side
re-filter (`StationSearchForm.tsx:43-67`, commented as necessary because the
suggestions are already server-filtered by both code and name, and Mantine's
default filter only checks `label`, i.e. the code — it would hide a correct
name-typed match). `renderOption` shows `"CODE — Name"` in the dropdown while
`data`'s `label` (what Mantine actually writes into the field on selection,
confirmed by that file's own comment reading Mantine's source) stays the bare
code. A "Look up" button resolves free-typed text against the live
`suggestions` array first by exact code, then exact name, then first
substring match, falling back to raw-text-uppercased only if nothing matched
(`StationSearchForm.tsx:15-38`) — so typing without ever opening the dropdown
still works.

`frontend/app/lines/CustomLineForm.tsx` reuses the identical pattern **three**
times in one form: a `TagsInput` "Operators" field via `useSuggestions(operatorsQuery, searchTocs)`
(`CustomLineForm.tsx:43-44,144-156`), an `Autocomplete` "Add station" field
via `useSuggestions(stationInput, searchStations)` (`CustomLineForm.tsx:46,157-190`,
same `filter`/`renderOption` shape as `StationSearchForm`), and a second
`TagsInput` "Destination CRS filter" via its own `useSuggestions(destinationQuery, searchStations)`
instance (`CustomLineForm.tsx:48-49,209-221`). Notably: **two independent
`useSuggestions` calls both hitting `searchStations`, side by side in the
same component** (`stationInput`/`stationSuggestions` and
`destinationQuery`/`destinationSuggestions`), proving the hook is safe and
intended to be instantiated once per field, not shared/coordinated across
fields — directly the shape `TrackTrainForm` would need for its two
independent CRS fields.

### Designing the equivalent for `TrackTrainForm`

**Can this reuse `useSuggestions`/`searchStations`/`searchTocs` verbatim?**
Yes. Nothing about `TrackTrainForm`'s three fields needs new plumbing:

- Origin CRS → `Autocomplete` + `useSuggestions(originCrs, searchStations)`,
  same shape as `StationSearchForm`'s single field.
- Destination CRS (optional) → a second, independent `useSuggestions(destinationQuery, searchStations)`
  instance, same shape as `CustomLineForm`'s two side-by-side station
  lookups.
- Operator (optional) → `useSuggestions(operatorQuery, searchTocs)`. Note
  `CustomLineForm`'s operator field is a `TagsInput` (multi-select, because a
  custom line can have several operators); `TrackTrainForm`'s Operator field
  is single-valued, so `Autocomplete` (single-select, same shape as the two
  CRS fields) is the right match, not `TagsInput` — there is no existing
  single-value-operator-Autocomplete precedent in this codebase, but it's a
  strict subset of what `CustomLineForm`'s `TagsInput` already does (same
  `useSuggestions`/`searchTocs` call, same `data`/`renderOption` mapping,
  just writing one string instead of appending to an array).

**Does validation need to change?** Only in relaxing the origin field's
*live* client-side gate, not in tightening anything server-side:

- `CRS_PATTERN` (`TrackTrainForm.tsx:12`) currently rejects the field as
  invalid (red error text, `TrackTrainForm.tsx:142`) for anything that isn't
  already exactly 3 letters — which would fire on every intermediate
  keystroke while typing a station *name* into an `Autocomplete` (e.g.
  "Wok" while typing "Woking"). `StationSearchForm`/`CustomLineForm` don't
  have this problem because their equivalent fields either don't validate
  live at all (`StationSearchForm`'s `Autocomplete` has no error state) or
  resolve through the same suggestion-matching function on submit
  (`CustomLineForm.tsx:69-89`'s `addStation`) rather than validating the raw
  input directly. `TrackTrainForm` would need the same shift: keep
  `CRS_PATTERN` only as a final resolved-value check (after selection or
  submit-time resolution), not as a live-while-typing error.
- The server-side check (`validate_pin`, `train_tracking.rs:47-69`) is
  unaffected either way — it only ever sees the final resolved 3-letter
  code, exactly as today.

**Does selecting a suggestion still resolve to the same plain strings
`TrackPinRequest` expects?** Yes, unchanged. Every existing autocomplete
field in this codebase (`StationSearchForm`, `CustomLineForm`'s three fields)
ultimately writes a bare CRS/ATOC code string into component state — never an
object, id, or anything richer. `TrackPinRequest`'s `origin_crs`/`destination_crs`/`operator`
are already exactly that shape (`crates/common/src/lib.rs:557-565`), so
`handleSubmit`'s body-construction (`TrackTrainForm.tsx:84-90`) needs no
change at all — only the three `useState<string>` values feeding it would now
be populated by selecting a suggestion (or by the same
exact-code/exact-name/first-match resolution `StationSearchForm.tsx:25-27`
and `CustomLineForm.tsx:79-86` already use, if the user types free text and
clicks/tabs away without opening the dropdown) instead of being typed
character-by-character as a raw code.

**A wrinkle `CustomLineForm`/`StationSearchForm` don't have: the pre-filled
origin.** `TrackTrainForm`'s origin field can arrive pre-filled —
`initialOrigin` (`TrackTrainForm.tsx:44-52`), set by `app/track/page.tsx:13,30`
from a `?origin=` query param when the user arrives via the "Track a train
from here" link on `/stations/[crs]` (per that page's own comment,
`TrackTrainForm.tsx:14-22`), used as the `originCrs` `useState` initial value
(`TrackTrainForm.tsx:52`). This is not actually a wrinkle in practice: both
`Autocomplete` and `TagsInput` in Mantine are controlled components that take
a plain string `value` exactly like `TextInput` does — nothing in the
existing autocomplete usages assumes an empty starting value, and a
pre-filled `originCrs` (e.g. `"WAT"`, already a valid resolved code from the
station page) would just render as the field's current text with no
suggestions dropdown open, identical to how `TextInput` renders it today.
The only behavior to preserve deliberately: don't fire a `searchStations`
call on mount just because the field starts non-empty — `useSuggestions`
already only searches in response to a query *change*
(`useSuggestions.ts:20-25`'s effect depends on `query`, and mounts do run the
effect on first render in React, so this needs a one-line check, e.g. skip
the initial fetch if the value came from `initialOrigin` unchanged, or simply
accept the harmless side effect of one suggestions fetch firing for the
pre-filled value on mount — either is a small, easily-tested implementation
choice, not a design blocker).

### Recommendation

**Yes — small, low-risk, high-value, worth doing.** Every piece of
infrastructure this needs already exists, is already proven in two other
forms in this exact codebase, and the design-time question of "should this
happen" was already answered "likely yes" and just never scheduled
(`2026-08-29-train-tracking-frontend-design.md:611-617`). The only real
implementation decisions are the two called out above (relax `CRS_PATTERN`
to a resolved-value check instead of a live-typing gate; make Operator a
single-value `Autocomplete` rather than copying `CustomLineForm`'s
`TagsInput`), both narrow and already precedented by existing code in this
repo.

Illustrative (not final) shape of the resulting diff to `TrackTrainForm.tsx`:

```tsx
// New imports, mirroring CustomLineForm.tsx/StationSearchForm.tsx exactly:
import { Autocomplete } from '@mantine/core';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';

// Three independent useSuggestions instances, one per field:
const { suggestions: originSuggestions } = useSuggestions(originCrs, searchStations);
const [destinationQuery, setDestinationQuery] = useState('');
const { suggestions: destinationSuggestions } = useSuggestions(destinationQuery, searchStations);
const [operatorQuery, setOperatorQuery] = useState('');
const { suggestions: operatorSuggestions } = useSuggestions(operatorQuery, searchTocs);

// Origin CRS: TextInput -> Autocomplete, same data/filter/renderOption shape
// as StationSearchForm.tsx:43-67. `originValid`/CRS_PATTERN move from a
// live onChange error to a resolved-value check at submit/selection time.
<Autocomplete
  label="Origin CRS code"
  value={originCrs}
  onChange={setOriginCrs}
  data={originSuggestions.map((s) => ({ value: s.code, label: s.code }))}
  filter={({ options }) => options}
  renderOption={({ option }) => { /* "CODE — Name", as elsewhere */ }}
  required
/>
// Destination and Operator: same pattern, Destination via searchStations,
// Operator via searchTocs, both optional (no `required`).
```

## Part 2: real trip/service search over the CIF SCHEDULE feed

### What exists today, precisely

`crates/schedule-reference` — confirmed directly by reading `parser.rs` in
full (384 lines) — parses exactly two CIF record types out of a delivered
`RJTTF*MCA.txt`/`RJTTF*MSN.txt` pair: `TI` (TIPLOC Insert, `parser.rs:17-64`)
and `A` (`parser.rs:66-` onward), producing a STANOX↔CRS reference table
(module doc, `parser.rs:1-13`; confirmed no `BS`/`BX`/`LO`/`LI`/`CR`/`LT`
handling anywhere in the file — grepped and found zero matches for those
record-type strings outside of comments describing what's *not* parsed). This
is pure location-reference data — "what CRS does STANOX X resolve to" — and
says nothing about which trains run, when, or between where.

This exact scoping question — "should full CIF timetable/schedule ingestion
happen now, alongside this narrower STANOX/CRS reference work" — was already
asked and answered concretely in
`docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md:495-543`
(Decision 5), against real, independently-verified record counts from the
same `timetable_full.zip` sample this codebase's own tests use:

> "**What full-timetable ingestion would actually require**... Parsing
> `BS`/`BX`/`LO`/`LI`/`CR`/`LT` — 8,610,939 of `MCA`'s 8,631,021 total lines
> [vs. `TI`+`A`'s 15,387 lines, 0.18% of the file]... **STP overlay
> resolution** (`C`/`N`/`O`/`P` precedence per calendar date)... a
> genuinely stateful algorithm, not a lookup... A materially larger schema:
> on the order of 400,000+ schedule rows and 6.8 million calling-point rows
> nationwide... If the goal is 'what runs between A and B'
> (`find_services`/`plan_journey`-shaped functionality), a journey-planning
> algorithm (RAPTOR or Connection Scan) on top of the parsed schedule
> data." (`2026-09-01-schedule-ingest-stanox-crs-table-design.md:501-529`,
> lightly excerpted)

That document goes on to cite a directly comparable, already-built reference
point — `train-mcp`'s own CIF timetable store plus a RAPTOR/Connection-Scan
journey planner, sized in `train-mcp`'s own design doc as "roughly a month of
work" for "26,848 schedules, 316,362 calling points, ~289,514 connections on
a measured weekday" from a similarly-sized CIF extract
(`2026-09-01-schedule-ingest-stanox-crs-table-design.md:530-542`), and
explicitly deferred full-timetable ingestion as out of scope
(`2026-09-01-schedule-ingest-stanox-crs-table-design.md:679-685`). This
document treats that sizing as directly transferable rather than
re-deriving it — the record counts and the "roughly a month" figure both
predate and are unaffected by the recent zip-delivery-pipeline fix
(`2026-09-03-schedule-feed-zip-delivery-correction.md`), which only changed
*how the raw files land on disk*, not what has to be built on top of them.

### What each unparsed record type actually encodes (CIF/RSPS5046)

For grounding "what would need to exist" concretely, not just as a line
count:

- **`BS` (Basic Schedule)** — the header of one train schedule: Train UID,
  the calendar range and days-of-week it runs (a bitmask), Bank Holiday
  running, Train Status, Train Category (e.g. ordinary passenger, express,
  freight), Train Identity (the headcode, e.g. `1A23`), Train Service Code,
  and — critically for date-filtering — the **STP indicator** (`P`ermanent /
  `O`verlay / `N`ew / `C`ancellation), which is what makes "does this train
  run today" a stateful precedence resolution across possibly-multiple `BS`
  records sharing a UID, not a single flat lookup.
- **`BX` (Basic Schedule Extra Details)** — a fixed continuation of `BS`:
  Traction Class, UIC Code, and the **ATOC/operator code** — the field a
  train's Operator would actually be sourced from.
- **`LO` (Origin Location)** — the train's starting point: TIPLOC, scheduled
  and public departure time, platform, line, allowances, activity code
  (e.g. "train begins").
- **`LI` (Intermediate Location)** — one calling or passing point between
  origin and terminus: TIPLOC, scheduled/public arrival and departure times,
  platform, line, activity code (e.g. "stops to set down and pick up
  passengers" vs. a pure pass-through).
- **`CR` (Change en Route)** — marks that from a given `LI` TIPLOC onward the
  train's own characteristics change (a new category, headcode, service
  code, or operator) — relevant to splitting/joining or through-working
  services; matters for correctness at the exact TIPLOC it occurs at, but is
  the smallest and most deferrable of the six for an MVP that only needs
  "what departs/calls at station X around time Y."
- **`LT` (Terminating Location)** — the train's final stop: TIPLOC, scheduled
  and public arrival, platform, activity code ("train finishes").

To answer "what trains run through station X around time Y" at all requires,
at minimum, `BS` (identity + which dates/days it runs — including STP
resolution to know if *today* is one of them), `BX` (operator, if the
Operator field is to be auto-filled from it), and whichever of
`LO`/`LI`/`LT` covers station X for that schedule (a station can be a train's
origin, an intermediate call, or its terminus — all three record shapes
carry a time at that TIPLOC). `CR` is only load-bearing if station X happens
to be a change-en-route point for a given service.

**Relative to `schedule-reference`'s existing `TI`/`A`-only parsing**: this is
not a small addition to that crate. `schedule-reference`'s parser is ~380
lines handling two flat, single-purpose record types with no cross-record
state. Full schedule parsing needs six record types, a UID-keyed
multi-record association (`BS`+`BX`+`LO`+N×`LI`+optional `CR`+`LT` all
belong to one schedule), STP overlay resolution across schedules sharing a
UID, and — per the prior research's count — roughly three orders of
magnitude more source lines to stream. This reads as its own new
crate/service (parallel to `schedule-reference`, not a module added to it),
with its own schema (a `schedules`/`calling_points` pair, per the prior
research's estimate of ~400,000+ and ~6.8M rows respectively) — consistent
with how the prior research already concluded (Decision 5, cited above), not
a new conclusion this document is reaching independently.

### What a search UX could look like, given that data

Three realistic shapes, each auto-filling a different subset of
`TrackTrainForm`'s four fields:

1. **"Type an origin station, see today's remaining departures, pick one."**
   User types/selects an origin CRS (reusing Part 1's exact autocomplete);
   the app queries "scheduled departures from X after now, today" and shows
   a short list (headcode, scheduled time, destination, operator). Picking
   one auto-fills **Origin, Scheduled departure, Destination, and Operator**
   — all four fields — since a single schedule row carries all of it. This
   is the most complete auto-fill, but also the shape most sensitive to STP
   resolution being right (an overlay/cancellation must be applied before
   showing "today's" list, or the list lies).

2. **"Type a headcode, see its scheduled calling pattern."** User types a
   headcode (e.g. `1A23` — already a first-class concept in this codebase's
   custom-line schema, `CustomLineForm.tsx:208`'s "Headcode prefixes" field);
   the app resolves it to today's matching `BS`/`BX`/`LO`/`LI`/`LT` chain and
   shows its full calling pattern, letting the user pick which calling point
   is "their" origin/destination. Auto-fills **Scheduled departure,
   Destination, Operator** once an origin call is picked; **Origin** is
   effectively chosen by the user picking a row in the pattern rather than
   typed separately. Requires headcode to already be a meaningfully unique,
   known-to-the-user identifier at search time — a real constraint (a
   headcode can be reused by different services on different days/routes,
   which is exactly what the UID+STP resolution exists to disambiguate).

3. **"Origin + operator + rough time window."** A narrower version of (1):
   user supplies origin CRS and (optionally) operator, gets back only
   scheduled departures within, say, ±2 hours of a typed time, rather than
   every remaining departure today. Auto-fills the same four fields as (1)
   once a row is picked, but the query itself is far cheaper to reason about
   (a bounded time window, not "everything from now to end of traffic day")
   and closest in shape to what a user is already doing today by hand
   (typing origin + operator + a departure time into the current form) — the
   search is validating/replacing manual entry, not adding a materially new
   capability on top of it.

### Does this reuse `useSuggestions`, or does it need something structurally different?

Structurally different, not a drop-in reuse — though the *shape of the
solution* (debounce, abort stale requests, render a picker) still applies
and `useSuggestions` itself could plausibly be reused as-is for the
mechanics. The difference is in the backend query, not the frontend hook:

- `searchStations`/`searchTocs` are simple prefix/substring text matches
  against a small, static-ish reference table (station names/codes, ATOC
  codes) — no time dimension, no "as of when" state, no per-day filtering.
- A trip/service search is a **time-windowed, calendar-filtered query**: "for
  *today's* date, resolve STP overlays to a concrete running/not-running
  answer per schedule, then filter to schedules calling at TIPLOC(s)
  matching CRS X within (now, now+window) or matching a headcode active
  today." That's a materially different query shape — closer to the
  RAPTOR/Connection-Scan-adjacent querying the prior research doc already
  flagged (`2026-09-01-schedule-ingest-stanox-crs-table-design.md:522-524`)
  than to a `LIKE '%query%'`-style text search. It would need its own
  backend endpoint and its own query logic; only the frontend
  debounce/abort mechanics are shared, and even those may need adjusting
  (e.g. a courser debounce, or no debounce at all if the UX is
  "pick a station, then browse a list" rather than "type and see live
  suggestions").

### Recommendation on scope/sequencing

**Not worth pursuing now**, and this document reaches that conclusion
independently rather than only citing the prior one, though it lands in the
same place. Three reasons, in order of weight:

1. **Part 1 already removes most of the actual pain this is aimed at
   cheaply.** The user-facing complaint this is solving — "typing raw
   3-letter codes by hand is annoying and error-prone" — is exactly what
   Part 1's autocomplete already fixes, for a small, low-risk, mostly-reuse
   change. Trip search additionally saves the user from knowing/typing the
   departure *time* and *operator*, which is real value, but it's
   incremental on top of Part 1, not a replacement for it.
2. **The infrastructure gap is large and already sized.** This isn't a
   guess — `2026-09-01-schedule-ingest-stanox-crs-table-design.md`'s
   Decision 5 already did this exact sizing exercise (six new record types,
   STP overlay resolution, a ~400K/~6.8M-row schema, a journey-planning-
   adjacent query layer, "roughly a month of work" by direct comparison to
   an equivalent already-built system) and explicitly deferred it. Nothing
   found in this session's reading changes that sizing — the recent
   zip-delivery-pipeline fix only fixed *file landing*, not parsing.
3. **Even the narrowest version (shape 3 above: origin + operator + time
   window) still requires the full `BS`/`BX`/`LO`/`LT` parsing chain and STP
   resolution** — there is no meaningfully smaller slice that avoids that
   core cost. "Just parse `LO`/`LT` and skip `BS`'s STP logic" would produce
   a schedule list that's wrong on any date an overlay or cancellation
   applies, which — given CIF schedules routinely carry base + overlay
   records — would show trains that aren't actually running today. That
   isn't a safe corner to cut for a feature whose entire value proposition
   is "trust what's shown enough to click it instead of typing."

**This needs its own dedicated design pass before any real recommendation on
building it — not because the picture is unclear, but because the prior
research already concluded exactly that**, and this document's own reading
of the same code and the same CIF record semantics doesn't surface anything
to overturn it. If it's revisited, the smallest genuinely-useful slice this
document can identify is shape (3) — origin + operator + a bounded time
window — since it most closely mirrors what a user already does by hand
today and would be the cheapest of the three shapes to validate for
correctness, but "cheapest of three expensive options" is not the same as
"cheap," and this document does not recommend scheduling it now.

## Open questions

1. **Part 1's Operator field**: should it stay a single-value `Autocomplete`
   as sketched, or is there a reason (not found in this session's reading)
   to prefer letting a user leave it blank and have the backend infer it
   later during resolution, rather than asking for it up front at all? Out
   of scope to answer here — `operator` is already optional in
   `TrackPinRequest` today regardless of input method.
2. **Whether `origin_crs`/`destination_crs` should gain server-side
   real-station validation** (rejecting a syntactically-valid-but-nonexistent
   3-letter code) is a related but separate question this document does not
   resolve — Part 1's autocomplete makes typing a bogus code far less likely
   but doesn't make it impossible (a user can still type free text and
   submit without selecting a suggestion, per `StationSearchForm`/
   `CustomLineForm`'s own fallback-to-raw-text behavior). Whether that gap is
   worth closing server-side is unaddressed here.
3. **Part 2's shape (2) (headcode search)** assumes a user often knows a
   train's headcode before tracking it — unverified against any real user
   behavior in this codebase (no analytics/usage data exists to check
   against). If that assumption is wrong, shape (2) is the weakest of the
   three sketched and shouldn't be prioritized first if/when this is
   revisited.
