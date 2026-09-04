# Track a Train — Picker Refactor: Reorder, All-Field Filtering, Always-Visible

**Status: approved design, ready to plan/implement.** Frontend-only, scoped
entirely to `frontend/components/TrackTrainForm.tsx` and its colocated test
file `frontend/components/TrackTrainForm.test.tsx`. No backend, Helm chart,
or other frontend file changes. Builds directly on the already-shipped
LDBWS/CIF picker (`docs/superpowers/specs/2026-09-03-trip-search-design.md`,
`docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md`) —
this document does not touch what data the picker fetches or from where,
only where it renders, what rows it shows given the form's current values,
and how it behaves before/around a successful fetch.

## Current state (exact)

`TrackTrainForm.tsx` render order today: Origin `Autocomplete` (label
"Origin CRS code") → picker (present in the DOM only when `picker !== null`,
line 271) → Scheduled departure `DateTimePicker` + "Now" button →
Destination `Autocomplete` (label "Destination CRS code (optional)") →
Operator `Autocomplete` (label "Operator (optional)") → field-error `Alert`
→ submit button.

The picker's fetch (`:120-150`) is keyed on `[originCrs, originValid]`
alone. Its rendering (`:271-350`) shows, in order: an `'unavailable'`
sentence, a "no live departures" sentence, the LDBWS row list, or the CIF
row list with its staleness disclaimer — nothing at all when
`picker === null` (either before the first fetch settles, or after a
non-404 error/network blip that intentionally leaves the picker absent
rather than guessing). `DepartureRow.destinationCrs`/`.operator` are plain
non-null strings; `ScheduleDepartureRow.destinationCrs` is `string | null`
and the type has no `operator` field at all (Decision 2,
2026-09-04-whole-network-trip-search-design.md).

## Goal

Three independent changes, all requested directly by the repo owner:

1. Move the picker to the end of the form (after Destination and
   Operator), and make its row filtering take Destination and Operator's
   current values into account, not just Origin.
2. Retitle Origin/Destination's labels so neither claims "CRS code" is the
   only valid input (name-based lookup already works for both).
3. Make the picker's container always present in the layout, with a
   sensible state for every point before/after a fetch, instead of
   popping in/out of the DOM.

## Decisions

### 1. Filtering semantics: exact-match-once-resolved, no filtering on partial text, and the CIF/Operator asymmetry handled by simply not applying that filter to CIF rows

**The core problem:** picker rows only ever carry a CRS code
(`destinationCrs`) and, for LDBWS rows only, an ATOC operator code
(`operator`) — never a station/operator *name*. Destination and Operator's
`Autocomplete` fields, meanwhile, hold whatever the user is currently
typing, which passes through several distinct phases: empty, a partial
name ("Wok"), a partial/typo'd code, a resolved 3-letter CRS code (typed
directly or chosen from a suggestion, indistinguishable once in the
field), and — for Operator — a resolved 2-letter ATOC code. There is no
name field on a row to substring-match a partial name against; attempting
that (e.g. testing whether "Wok" is a substring of a row's destination
code "WOK") would work by pure accident for stations whose code happens to
be a prefix of their name and mislead for every other station.

**Decision: filter only once the relevant field looks like a resolved
code; apply no filtering at all while it still looks like in-progress
free text.** Reuses the exact 3-letter `CRS_PATTERN` already used for
Origin's own validation for Destination; a new, parallel 2-letter
`OPERATOR_PATTERN` for Operator (ATOC codes are consistently 2 letters
across this codebase — `crates/common/src/lib.rs`'s `operator: String`
fields, `Suggestion.code` values from `/public/tocs`, and every reference
to `"SW"`-shaped codes in existing tests and fixtures — there is no
existing named constant for this, so this document introduces one,
mirroring `CRS_PATTERN`'s own shape and role exactly):

```tsx
const OPERATOR_PATTERN = /^[A-Za-z]{2}$/;

/** True unless `destinationCrs` looks like a resolved 3-letter code AND
 * the row's own destination doesn't case-insensitively match it. While
 * the field still holds partial/typed-name text (or is empty), every row
 * matches -- there is nothing on a row to honestly match partial text
 * against (rows carry a CRS code, never a station name). A `null` row
 * destination (CIF only) never matches an *active* filter: "unknown"
 * is not "assume it matches". */
function matchesDestination(rowDestinationCrs: string | null, destinationCrs: string): boolean {
  const trimmed = destinationCrs.trim();
  if (!CRS_PATTERN.test(trimmed)) return true;
  return rowDestinationCrs !== null && rowDestinationCrs.toUpperCase() === trimmed.toUpperCase();
}

/** Same idea for Operator, LDBWS rows only -- see Decision 1's CIF/Operator
 * point below for why CIF rows never call this at all. */
function matchesOperator(rowOperator: string, operator: string): boolean {
  const trimmed = operator.trim();
  if (!OPERATOR_PATTERN.test(trimmed)) return true;
  return rowOperator.toUpperCase() === trimmed.toUpperCase();
}
```

Both are pure functions of a row field and the corresponding form field —
no new state, computed fresh each render alongside the existing `picker`
read. Case-insensitivity matters because typed text and a picked
suggestion could differ in case even though `Autocomplete` values from
this codebase's own suggestion sources are already consistently
upper-cased; comparing case-insensitively removes any dependency on that
staying true.

**Why "exact match once resolved, nothing while typing" over the
alternatives considered:**

- *Substring/prefix match against the row's code the whole time* — rejected
  above: only coincidentally correct, actively misleading in the general
  case, because rows have no name field to match against.
- *Resolve typed text against the live `destinationSuggestions`/
  `operatorSuggestions` array first, then filter by the resolved code* —
  rejected as unnecessary complexity for this slice: it would let the
  *filter* jump around based on suggestion-list ordering/timing
  independent of what's literally in the field, is a heavier behavior
  change than "let the field's already-existing valid/resolved state drive
  filtering," and the codebase's own precedent for this kind of resolve
  (`StationSearchForm`'s "Look up" button, `CustomLineForm`'s "Add"
  button — see `2026-09-03-track-a-train-autocomplete-design.md` Decision
  4's "Rejected alternative") is explicit-action-triggered, not a live
  filter. Once the user has actually picked/typed a resolved code, exact
  match is unambiguous and needs no resolution step.
- *No filtering at all until the field is "complete" by some fuzzier
  heuristic (e.g. length >= 3 for a name)* — rejected because it has no
  clean signal and would filter inconsistently station-to-station (a
  3-letter code and a 3-letter-prefix-of-a-name are indistinguishable by
  length alone); `CRS_PATTERN`/`OPERATOR_PATTERN` are the one signal the
  form already trusts elsewhere for "this field holds a resolved code."

**The CIF/Operator schema asymmetry — the one genuinely non-obvious call
in this document:** `ScheduleDepartureRow` has no `operator` field at all
(the CIF SCHEDULE feed doesn't carry one — confirmed by the type itself
and the whole-network design doc's Decision 2). An Operator filter
therefore cannot be *evaluated* against a CIF row in any meaningful sense
— there is no value to compare. Two ways to handle that were considered:

1. **Treat "no operator field" as "never matches"** — an active Operator
   filter would hide every CIF row outright. Rejected: this would make
   typing anything into Operator silently defeat the entire point of the
   CIF fallback (Decision 3 of the whole-network design doc: the CIF
   picker exists specifically to cover stations with zero LDBWS data) —
   the user would see the picker vanish for a reason that has nothing to
   do with the CIF rows' own destinations/times being wrong, just an
   unrelated field they filled in.
2. **Treat "no operator field" as "the filter doesn't apply to this row" —
   CIF rows are simply exempt from the Operator filter, always passing it
   regardless of what's typed.** Adopted. `matchesOperator` is never even
   called for CIF rows — the CIF branch's filter predicate only ever
   calls `matchesDestination`. This is the literal, honest reading of "a
   filter over a field that doesn't exist on this row": it's not that the
   row fails to match, it's that the question doesn't apply, so it can't
   fail. It also means a user who has typed both a Destination and an
   Operator, and lands on a CIF-fallback station, still gets a
   Destination-filtered CIF list rather than an empty one — the filtering
   that *can* honestly apply (Destination, which `ScheduleDepartureRow`
   does carry) still does.

**Whether Destination filtering should apply to CIF rows: yes.**
`ScheduleDepartureRow.destinationCrs` exists (nullable, per the
whole-network design doc's Decision 2) — there is a real value to compare,
so the same `matchesDestination` used for LDBWS rows applies unchanged. A
`null` row destination never matches an *active* Destination filter (the
function's own doc comment) — this can legitimately reduce a CIF list to
zero rows when every remaining departure's destination is unmapped or
genuinely doesn't match, and that's correct behavior, not a bug to work
around: unlike the Operator case, this filter is evaluating a real field
the CIF row actually has, so "no match" is an honest answer, not a schema
gap being misread as a mismatch.

### 2. Render order: picker moves to the end, immediately before the field-error alert

New order: Origin → Scheduled departure → Destination → Operator → picker
→ field-error `Alert` → submit button. This makes the picker genuinely
reflect "all fields entered so far" by the time it renders — Destination
and Operator's current values, which the filter in Decision 1 reads, are
already on-screen above it, rather than the picker sitting between Origin
and the fields whose values it can't yet consider. No field's own
position among Origin/Scheduled departure/Destination/Operator changes
relative to each other — only the picker moves.

### 3. Labels: drop "CRS code" from Origin and Destination, keep the "(optional)" suffix convention

- Origin: `"Origin CRS code"` → **`"Origin station"`**
- Destination: `"Destination CRS code (optional)"` → **`"Destination
  station (optional)"`**
- Operator: unchanged (`"Operator (optional)"` already says nothing about
  "code").

Placeholders (`"e.g. Woking or WOK"`) are unchanged — they already
demonstrate name-or-code without asserting either is the *only* valid
form, so nothing about them contradicted the old label; only the label
text itself claimed more than is true. The inline validation error text
("Must be a 3-letter CRS code") is left as-is: it fires only once a
non-empty, non-blank value fails the pattern, at which point it's
correctly describing the concrete rule being enforced, not describing
what's "valid to type" in general — different scope from the label, not
addressed by this document's brief.

### 4. Always-visible picker: one persistent container, one of six mutually-exclusive states, a fixed `min-height` to blunt (not eliminate) size jumps

The picker's outer element renders unconditionally now — never absent
from the DOM — with its *content* switched on a `pickerContent()` render
function evaluated in this priority order (first match wins):

1. **`!originValid`** (empty or not-yet-a-valid-code Origin) — prompt
   state: *"Enter an origin station above to see upcoming departures."*
   This is the new default state a user sees on landing at `/track` with
   nothing filled in yet — deliberately worded as guidance, not an error
   (no red text, no `Alert`), matching this form's existing `dimmed` `Text`
   convention for every other picker sentence.
2. **`pickerLoading`** (a new boolean, Decision 5) — *"Checking for
   departures…"* Covers both the brief window after Origin resolves and
   before the first fetch settles, and every subsequent re-fetch after
   Origin changes to a different valid code — without this, a stale
   previous station's rows (or nothing at all, pre-fetch) would show
   during that window; see Decision 5.
3. **`picker === null`** (origin valid, fetch settled, but the LDBWS call
   errored non-404 or the request failed outright — the existing
   "leave the picker absent" outcome, `:144`/`:148` today) — *"Couldn't
   load departures for this station right now — enter the details
   below."* New copy; today this state renders nothing at all. Distinct
   from state 4 (`'unavailable'`) on purpose: `null` means "we don't know
   whether this station has data, the request itself failed"; `'unavailable'`
   means "we asked both sources and confirmed neither has anything" — an
   honest difference worth two different sentences.
4. **`picker === 'unavailable'`** — unchanged copy: *"No departure
   information is available for this station — enter the details below."*
5. **`picker.rows.length === 0`** (the *unfiltered* fetch result was
   empty — a real, un-narrowed empty board/timetable) — unchanged copy:
   *"No live departures currently on the board for this station right
   now."* Checked against the raw fetched rows, not the filtered set, so
   this sentence keeps meaning exactly what it says today: the source
   itself had nothing, as opposed to state 6 below.
6. **Filtered rows, by source:**
   - `source === 'ldbws'`: rows filtered by `matchesDestination` AND
     `matchesOperator` (Decision 1). If the filtered set is empty while
     the *unfiltered* set wasn't (a new, seventh sub-case) — *"No upcoming
     departures match the destination and/or operator you've entered."*
     Otherwise, the existing row list/badges, unchanged rendering, applied
     to the filtered array instead of the full one.
   - `source === 'cif'`: rows filtered by `matchesDestination` only
     (Decision 1's CIF/Operator asymmetry). Same "filtered to zero but
     source had rows" case gets its own sentence, appended below the
     existing staleness disclaimer (which always renders for this source,
     filtered-to-zero or not, since it's a property of the *source*, not
     of how many rows survived filtering): *"No upcoming scheduled
     departures match the destination you've entered."* Otherwise, the
     existing row list, unchanged rendering, applied to the filtered
     array.

Row click handlers (`pickDeparture`/`pickCifDeparture`) are unchanged —
filtering only ever changes which rows are *shown*, never the data a
clicked row fills in.

**Size-jump mitigation:** the persistent container gets a fixed
`mih={72}` (Mantine's `min-height` style shorthand) — enough for the
one/two-line sentence states (1-3, 4, 5, and the two new "filtered to
zero" sentences) to occupy consistent space, so switching between them
causes no layout shift at all. Row-list states (6) are already bounded
above by the existing `ScrollArea mah={220}` and can legitimately grow
past the minimum height for a handful of rows — a full-network fixed
height matching the tallest possible list isn't attempted (would waste
large amounts of vertical space in the far more common few-sentence
states) per the brief's own "use your judgment, a min-height container is
a reasonable, low-risk way to reduce jumpiness without overengineering
it." This bounds the jump in one direction (text states never shift
against each other) without claiming to eliminate the other (a handful of
rows appearing is still a visible, but bounded and expected, size change
— the point is that it's never a *surprise pop-in of the section itself*,
which is what the brief's UX complaint was actually about).

### 5. `pickerLoading`: a new boolean, set around the existing fetch effect, guarded against the abort race

```tsx
const [pickerLoading, setPickerLoading] = useState(() => CRS_PATTERN.test(initialOrigin.trim()));
```

Initialized from `initialOrigin` directly (not `false`) so a form mounted
with an already-valid pre-filled origin (the `/track?origin=WAT` case,
`initialOrigin` prop from a station page or ticket flow) shows "Checking
for departures…" on the very first paint rather than flashing state 3
("Couldn't load departures…") for one render before the effect's
`setPickerLoading(true)` runs.

The existing effect gains `setPickerLoading(true)` right after the
`!originValid` early return, and `setPickerLoading(false)` in every
terminal branch of the `.then` chain (unavailable, null, cif, ldbws) —
each of those branches already exists and already calls `setPicker(...)`;
this document adds one paired call alongside each, not new branches. The
`.catch` (unchanged posture: "aborted or network blip — leave prior
`picker` state") gets one guard:

```tsx
.catch(() => {
  if (!controller.signal.aborted) setPickerLoading(false);
});
```

Without this guard, a fast second origin change (which aborts the first
effect's `controller` and starts a second effect run) would let the
*first* effect's now-rejected fetch's `.catch` fire after the *second*
effect has already set `pickerLoading` back to `true` — incorrectly
flipping it to `false` mid-flight for the second, still-in-progress fetch.
Checking `controller.signal.aborted` (the closure's own controller, not
some shared/global flag) distinguishes "this specific request was
superseded" from "this specific request genuinely failed," the same
per-effect-instance pattern the existing cleanup function
(`return () => controller.abort()`) already relies on.

## Explicitly out of scope

- **Backend query-string filtering.** Both `/departures` and
  `/schedule-departures` return a station's full row set unconditionally
  today, and continue to. Station departure lists are dozens of rows, not
  thousands (confirmed by the existing `MAX_DEPARTURES_PER_STATION = 10`
  cap on the CIF side and LDBWS's own `num_rows` poller default,
  `2026-09-04-whole-network-trip-search-design.md` Decision 1) — filtering
  a few dozen already-fetched rows client-side on every keystroke is not a
  performance concern this document found any reason to route through the
  network instead.
- **Resolving typed-but-unmatched Destination/Operator text against
  suggestions before filtering.** Same reasoning as Decision 1's rejected
  alternative — this document's filter reads the field's literal current
  value through the same `CRS_PATTERN`/`OPERATOR_PATTERN` gate the rest of
  the form already trusts, not a resolved-via-suggestions value.
  Deferred, matching the same instinct that has now been applied twice
  (`2026-09-03-track-a-train-autocomplete-design.md` Decision 4's original
  "Rejected alternative", and here again for filtering rather than
  submission).
- **Any change to `pickDeparture`/`pickCifDeparture`, `TrackPinRequest`,
  or `POST /Train/track`.** Filtering changes only which rows render;
  clicking a shown row fills fields exactly as before.
- **A loading spinner/skeleton widget.** `pickerLoading`'s "Checking for
  departures…" state is plain `dimmed` text, matching every other picker
  sentence's visual weight — no new dependency, no new visual language
  introduced for one transient state.
- **Debouncing the Destination/Operator filter itself.** Unlike the
  `useSuggestions` network fetches (debounced 250ms because they hit the
  backend), this filter runs client-side over already-fetched rows on
  every render — no debounce is needed or added; it's cheap array
  filtering over dozens of rows, not a network call.

## Testing changes required

- Every existing test that looks up Origin/Destination by their old label
  text (`getByRole('combobox', { name: /Origin CRS code/ })` /
  `/Destination CRS code/`) must be updated to the new label text
  (`/Origin station/` / `/Destination station/`) — mechanical, no
  behavior-under-test changes for the large majority of these.
- Tests that assert the picker section is *absent* from the DOM before a
  valid Origin (none exist explicitly today — the old code simply never
  rendered anything, so there was nothing to assert against) instead gain
  new coverage: the picker container is present and shows the new prompt
  sentence when Origin is empty/invalid.
- New coverage for Decision 1's filtering: Destination narrows LDBWS rows
  once it holds a resolved 3-letter code (not while still partial);
  Operator narrows LDBWS rows the same way; an Operator filter does not
  eliminate CIF rows (CIF/Operator asymmetry); a Destination filter can
  legitimately eliminate CIF rows down to the "no upcoming scheduled
  departures match" sentence when every row's destination is null or
  non-matching.
- The existing filtering-adjacent tests (`'renders a cancelled and an
  on-time departure...'`, `'clicking a non-cancelled row...'`, `'changing
  the origin away...'`) all run with Destination/Operator empty at the
  point they assert on picker content, so `matchesDestination`/
  `matchesOperator` are no-ops for them (every row matches when the
  corresponding field is empty) — these are expected to keep passing
  unchanged in behavior, only touched if the render-order move affects
  how they locate elements (it doesn't; all locate by role/text, not DOM
  position).
