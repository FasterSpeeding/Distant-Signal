# Track a Train — Field Autocomplete Design

**Status: approved design, ready to plan/implement.** Turns Part 1 of
`docs/superpowers/specs/2026-09-03-track-a-train-input-ux-research.md`
("do it, small and low-risk") into an exact, buildable spec for
`frontend/components/TrackTrainForm.tsx`. Part 2 of that research
(real trip/service search) is explicitly not this document's concern —
that research recommended *against* scheduling it now, and nothing here
depends on it.

No backend change. Confirmed by the research doc and re-confirmed here:
`common::TrackPinRequest` (`crates/common/src/lib.rs:557-565`) and
`POST /Train/track`'s `validate_pin` (`crates/api/src/data/train_tracking.rs:47-69`)
both already accept `origin_crs`/`destination_crs`/`operator` as plain
strings with no awareness of how the frontend obtained them. This document
touches `frontend/components/TrackTrainForm.tsx` and its colocated test
file only.

## Current state (exact)

`TrackTrainForm.tsx:44-56` holds three relevant `useState<string>` values:
`originCrs` (initialized from `initialOrigin`, `:52`), `destinationCrs`
(`:53`), `operator` (`:54`). Rendered as three plain `TextInput`s
(`:137-144`, `:174-179`, `:180-185`).

Validation today (`TrackTrainForm.tsx:12,60-61`):

```tsx
const CRS_PATTERN = /^[A-Za-z]{3}$/;
const originValid = CRS_PATTERN.test(originCrs.trim());
const canSubmit = originValid && scheduledDeparture !== null && !submitting;
```

`originValid` is recomputed on every render directly from `originCrs`, and
its negation drives both the inline field error (`:142`,
`error={originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}`)
and the submit-button gate (`:192`, `disabled={!canSubmit}`) — there is no
separate "resolved" state; the raw typed string *is* the validated value.
`destinationCrs`/`operator` have no client-side validation at all today.

On submit (`:84-90`), the three strings are trimmed/uppercased (origin and
destination only — `operator` is trimmed but not uppercased) and folded
into `TrackPinRequest`, `destination_crs`/`operator` included only if
non-empty after `.trim()`.

`TrackTrainForm.test.tsx:66-69` currently asserts the live-typing error
fires for a non-3-letter origin (`'WATERLOO'`) — this test's assertion is
exactly the behavior Decision 1 below removes, and must be rewritten, not
just left in place expecting it to still pass.

## Decisions

### Decision 1 — Origin CRS: `TextInput` → `Autocomplete`, validation moves from live-typing to resolved-value

Replace the `TextInput` with a Mantine `Autocomplete`, wired exactly like
`StationSearchForm.tsx:43-67`:

```tsx
const [originQuery, setOriginQuery] = useState(initialOrigin);
const { suggestions: originSuggestions } = useSuggestions(originQuery, searchStations);
```

`originCrs` (the value actually submitted) and `originQuery` (what
`useSuggestions` debounces against) are **the same value** — this field
has no separate "committed" vs. "in-progress-typing" state, matching
`StationSearchForm`'s single `crs` state exactly (that form has one
state variable serving both roles). So in practice this is just:

```tsx
const [originCrs, setOriginCrs] = useState(initialOrigin);
const { suggestions: originSuggestions } = useSuggestions(originCrs, searchStations);
```

— no new state variable at all, `useSuggestions` fed directly by the
existing `originCrs` state. (`CustomLineForm`'s stations/destination
fields use a *separate* query state only because their committed value is
an array — `TagsInput`'s `onSearchChange` vs. `onChange` are genuinely
different things there. `Autocomplete`'s `onChange` fires on every
keystroke *and* on selection, so one state variable already captures
both, same as `StationSearchForm`.)

```tsx
<Autocomplete
  label="Origin CRS code"
  placeholder="e.g. Woking or WOK"
  value={originCrs}
  onChange={setOriginCrs}
  data={originSuggestions.map((s) => ({ value: s.code, label: s.code }))}
  filter={({ options }) => options}
  renderOption={({ option }) => {
    const match = originSuggestions.find((s) => s.code === option.value);
    return match ? `${match.code} — ${match.name}` : option.value;
  }}
  error={originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
  required
/>
```

**What "resolved value" means, concretely — the actual open question the
research doc left unanswered:**

Keep `originValid = CRS_PATTERN.test(originCrs.trim())` — the same regex,
computed the same way, over the same state variable — unchanged. Nothing
about *how validity is computed* changes. What changes is *when the user
sees an error for it*, and that's a UI-only fix already implied by
switching to `Autocomplete`: because typing a station name ("Woking")
routes through the same `onChange`/state as before, the live error would
still fire on "Wok" today. The fix is not "resolve against suggestions
before validating" (that would be a heavier design — see the rejected
alternative below) — it's the same regex, evaluated after a short pause
in typing rather than on every keystroke, using the field's blur event
plus the existing debounce as the signal that the user is done typing:

```tsx
const [originTouched, setOriginTouched] = useState(false);
// ...
<Autocomplete
  // ...
  onBlur={() => setOriginTouched(true)}
  error={originTouched && originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
/>
```

This is a **direct, minimal fix**: the error already only ever depended
on `originCrs`/`CRS_PATTERN`; gating it on `onBlur` (a field the user has
left) rather than every `onChange` (a field being actively typed into)
removes the false-positive flash while typing a name, with no new
resolution logic, no new state shape, and no change to what "valid"
means. `canSubmit` stays keyed to the ungated `originValid` (not
`originTouched`) — a user who never blurs the field (e.g. tabs by
keyboard through to the date picker, which does blur it, or clicks
straight to Submit) must still be blocked from submitting an unresolved
value; only the *displayed error text* is gated on touch. Note: this
codebase has no existing "touched" field precedent to point to (grepped
`frontend/` for `onBlur`/`Touched` — no hits outside this document) — this
is a new, small, standard-React pattern introduced here, not a reuse of
an existing one. It's called out explicitly rather than glossed over
because it's the one genuinely new piece of state/behavior in this
document; everything else in Decisions 1–3 is a direct copy of an
existing field's shape.

**Rejected alternative — resolve against the live `suggestions` array
before validating (`StationSearchForm.tsx:25-27`'s exact-code/exact-name/
first-match chain):** This is what `StationSearchForm`'s "Look up" button
and `CustomLineForm`'s "Add" button both do, but only at an explicit
user *action* (a button click), not as a live `onChange`/error-text gate.
Reusing it here for `originValid` would mean the field's committed value
changes to something the user didn't literally type (e.g. typing "Wokin"
and having it silently coerce to `"WOK"` because that's `suggestions[0]`)
merely by pausing — a heavier, more surprising behavior change than the
research doc scoped, and not what either existing precedent does for a
*live* field (both existing resolution call sites are explicit-action
triggered: a button, not a blur/pause). Deferred, not adopted — see
Explicitly out of scope.

### Decision 2 — Destination CRS: `TextInput` → `Autocomplete`, optional, no validation (unchanged from today)

```tsx
<Autocomplete
  label="Destination CRS code (optional)"
  placeholder="e.g. Woking or WOK"
  value={destinationCrs}
  onChange={setDestinationCrs}
  data={destinationSuggestions.map((s) => ({ value: s.code, label: s.code }))}
  filter={({ options }) => options}
  renderOption={({ option }) => {
    const match = destinationSuggestions.find((s) => s.code === option.value);
    return match ? `${match.code} — ${match.name}` : option.value;
  }}
/>
```

fed by its own independent `useSuggestions(destinationCrs, searchStations)`
instance — same one-state-variable shape as Decision 1, and the same
"two independent `useSuggestions` calls both hitting `searchStations`,
side by side in one component" pattern the research doc already found
proven safe in `CustomLineForm.tsx:46,49`.

No validation added — `destinationCrs` has none today (research doc,
"current form, exactly as it stands") and this document doesn't add any.
`canSubmit` is unaffected by this field, unchanged from today.

**Empty/cleared must submit as absent, not as an empty-string mismatch.**
This already works, unchanged: `handleSubmit`'s
`...(destinationCrs.trim() ? { destination_crs: ... } : {})` (`:88`) spreads
in nothing at all when the trimmed value is empty — the key is omitted
from the request body entirely, not sent as `""`. Nothing about
`Autocomplete` changes this: selecting no suggestion, or clearing a
selected one (Mantine's `Autocomplete` supports clearing the field back
to `''` by deleting the text — there's no separate "select nothing"
affordance needed since it's a plain controlled string, not an object
value), still leaves `destinationCrs === ''`, which still fails
`.trim()`'s truthiness check exactly as it does today. This is worth
stating explicitly (per this document's brief) precisely because it's
easy to assume an `Autocomplete` needs a `null`-able "cleared" state the
way a `Select` with object values might — it doesn't, because this
`Autocomplete`'s value type was already, and remains, a plain string.

### Decision 3 — Operator: `TextInput` → single-value `Autocomplete` (not `TagsInput`), optional, no validation

```tsx
<Autocomplete
  label="Operator (optional)"
  placeholder="e.g. SW"
  value={operator}
  onChange={setOperator}
  data={operatorSuggestions.map((s) => ({ value: s.code, label: s.code }))}
  filter={({ options }) => options}
  renderOption={({ option }) => {
    const match = operatorSuggestions.find((s) => s.code === option.value);
    return match ? `${match.code} — ${match.name}` : option.value;
  }}
/>
```

fed by `useSuggestions(operator, searchTocs)`. Confirms the research
doc's conclusion directly: `TrackPinRequest.operator` is `Option<String>`
(`crates/common/src/lib.rs:564`) — one string, not a set — so a tracked
train has exactly one operator, and `CustomLineForm`'s `TagsInput`
"Operators" field (`CustomLineForm.tsx:144-156`, feeding `operators:
string[]`, a genuinely multi-valued custom-line concept) is the wrong
shape to copy. `Autocomplete` here is a strict subset of that field's own
`useSuggestions`/`searchTocs` call and `data`/`renderOption` mapping —
same suggestion source, same rendering, writing one string via `onChange`
instead of appending to an array via `TagsInput`'s `onChange`/
`onSearchChange` split.

Submission unchanged: `...(operator.trim() ? { operator: operator.trim() } : {})`
(`:89`) already omits the key for an empty string, same reasoning as
Decision 2. No case-normalization added — `operator.trim()` today does
**not** `.toUpperCase()` the way `origin_crs`/`destination_crs` do
(`:86,88` vs. `:89`); this document preserves that asymmetry rather than
"fixing" it, since it's out of this document's stated scope and ATOC
codes selected from `searchTocs` suggestions already arrive
correctly-cased from the backend (`reference.rs`'s `Suggestion.code`),
same as the CRS fields' suggestions do.

### Decision 4 — Fallback to raw, unresolved text on submit: allowed, matching existing precedent exactly

**The question the brief asks directly: does a user who types a value
that never matches any suggestion get blocked from submitting, or can
they submit raw text as a fallback?**

Raw text is allowed through, for all three fields, unchanged from
today's behavior — this is not a new decision, it's the *absence* of a
new gate. Checked against both existing precedents named in the brief:

- `StationSearchForm.tsx:15-38`'s "Look up" button resolves free-typed
  text against live suggestions (exact code → exact name → first
  substring match) and **falls back to the raw text uppercased** only if
  nothing matched (`:27`, `suggestions[0]?.code ?? trimmed.toUpperCase()`)
  — it never blocks the navigation over an unmatched query.
- `CustomLineForm.tsx:69-89`'s `addStation` does the identical fallback
  chain (`:81`) before its one remaining gate is just length (`crs.length
  !== 3`, `:86`) — again, no "must match a real suggestion" gate.

`TrackTrainForm` already works this way today, before any of this
document's changes, and continues to unchanged after them: `originCrs`'s
only gate is `CRS_PATTERN` (a syntactic 3-letter check), not "is this one
of `originSuggestions`". A user who types `"XXX"` (syntactically valid,
not a real station — the research doc already notes the backend doesn't
check this either, `train_tracking.rs:47-69`) can submit exactly as they
can today. `destinationCrs`/`operator` have no format gate at all, exactly
as today. This document does not add a "must resolve to a suggestion"
requirement to any of the three fields — doing so would be new,
stricter-than-precedent behavior this document's scope (a UX/reuse
change, not a validation-tightening change) doesn't call for, and would
contradict both cited precedents' own explicit fallback design.

### Decision 5 — the pre-filled `initialOrigin` case: no special-casing needed

Per the research doc's own conclusion (which this document re-confirms
rather than revisits): `originCrs`'s initial value stays
`useState(initialOrigin)`, unchanged. `useSuggestions`'s effect
(`useSuggestions.ts:20-52`) does run on mount because `query` (here,
`originCrs`) is non-empty from the first render, firing one
`searchStations` call for whatever `initialOrigin` was (e.g. `"WAT"`) —
this is accepted as a harmless side effect, not special-cased away
(`useSuggestions.ts` has no mount-vs-change distinction to hook into
without changing the shared hook itself, which is out of scope — see
Explicitly out of scope). The resulting suggestions dropdown simply isn't
opened unless the user interacts with the field, matching
`StationSearchForm`/`CustomLineForm`'s own behavior for any non-empty
initial value.

## Testing changes required (not new infrastructure — updates to existing coverage)

`TrackTrainForm.test.tsx:66-69` ("shows a field error for a non-3-letter
origin code") currently drives the error purely via `fireEvent.change`
and asserts the error text appears immediately. Per Decision 1, the error
is now gated on blur, so this test must add a `fireEvent.blur` after the
`fireEvent.change` before asserting the error text — otherwise it fails
against Decision 1's intended behavior (no live-while-typing error), not
because the fix is wrong. Every other existing test in that file
(`initialOrigin` pre-fill, submit-disabled-until-valid, all the
POST/redirect/error-handling tests) drives the fields via
`fireEvent.change` on `getByLabelText`, which continues to work
unchanged — Mantine's `Autocomplete`, like `TextInput`, renders as a
labelled `<input>` element, so no test call-site beyond the one above
needs to change shape, only the one assertion named here.

## Explicitly out of scope

- **Resolving typed-but-unmatched text against suggestions before
  submit** (the `StationSearchForm`/`CustomLineForm` "Look up"/"Add"
  button pattern). Decision 4 confirms raw text already passes through
  unresolved today and continues to; adding an explicit resolve step
  would be new behavior beyond a reuse-and-adapt change, and there is no
  button in this form's design to trigger it from (submit already does
  the job the "Look up"/"Add" buttons do elsewhere).
- **Server-side real-station validation** of `origin_crs`/`destination_crs`
  against the actual reference set. Named as a separate open question by
  the research doc (Open Question 2) and not resolved here — this
  document's autocomplete makes typing a bogus code far less likely but,
  per Decision 4, does not make it impossible, exactly as intended.
- **Changing `useSuggestions` itself** (e.g. adding a mount-vs-change
  distinction) to avoid the one harmless extra fetch on a pre-filled
  origin (Decision 5). `useSuggestions.ts` is shared by every other
  autocomplete field in this codebase; a behavior change there is a
  bigger blast radius than this form warrants for a single accepted,
  harmless side effect.
- **Case-normalizing `operator`** to match `origin_crs`/`destination_crs`'s
  existing `.toUpperCase()` on submit (Decision 3's note on the existing
  asymmetry). Pre-existing behavior, not touched by this change.
- **Part 2 of the research doc** (real trip/service search). Unrelated
  scope, already recommended against scheduling now by that document's
  own conclusion.
- **Any change to `TrackPinRequest`, `validate_pin`, or any other backend
  code.** Confirmed unnecessary above and by the research doc; this is a
  frontend-only document.

## Open questions

None blocking implementation. The one item worth flagging for a future
pass, not this one: Decision 1's `onBlur`-gated error means a user who
tabs directly from the Origin field to the next control without ever
typing anything (leaving it empty) will see no error text even though
`required` blocks submission — the error condition is already guarded by
`originCrs.length > 0` (unchanged from today), so an empty field silently
relies on the disabled Submit button alone, same as it does today for
every other required-but-empty field in this form. Not a regression this
document introduces, just worth flagging in case a future accessibility
pass wants an explicit "this field is required" message on
blur-while-empty across the whole form, not just this field.
