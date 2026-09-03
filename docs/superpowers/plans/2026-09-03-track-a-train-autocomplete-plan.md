# Track a Train — Field Autocomplete — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.
>
> **Single task, frontend-only.** No backend change, no cross-task
> dependency, no sequencing concerns — everything lands in one commit
> against one file (plus its colocated test file).

**Goal:** implement
`docs/superpowers/specs/2026-09-03-track-a-train-autocomplete-design.md`
end to end — give `TrackTrainForm.tsx`'s Origin CRS, Destination CRS, and
Operator fields the same `Autocomplete`/`useSuggestions` pattern already
proven in `StationSearchForm.tsx` and `CustomLineForm.tsx`, relax Origin's
validation from a live-while-typing error to a blur-gated one, and make
Operator a single-value `Autocomplete` (not a `TagsInput`).

**Architecture:** zero new files, zero new shared infrastructure. Reuses
`frontend/lib/useSuggestions.ts` and `frontend/lib/suggestions.ts`
(`searchStations`/`searchTocs`) exactly as they already exist. Only
`frontend/components/TrackTrainForm.tsx` and
`frontend/components/TrackTrainForm.test.tsx` change.

**Tech Stack:** Next.js 16 App Router + TypeScript, Mantine 8
(`Autocomplete`), Vitest 2 + `@testing-library/react`
(`frontend/test/render.tsx`'s `renderWithMantine` helper).

**Design doc:**
`docs/superpowers/specs/2026-09-03-track-a-train-autocomplete-design.md`
— its Decisions section is authoritative for every markup/state/
validation detail below; this plan does not repeat the reasoning, only
the concrete steps.

---

## Non-goals

- **No backend change.** `common::TrackPinRequest` and `validate_pin`
  already accept plain strings regardless of input method (design doc,
  confirmed from the research doc). Nothing in `crates/` changes.
- **No resolve-unmatched-text-before-submit step.** Design doc Decision 4:
  raw, unresolved text is allowed through on submit for all three fields,
  matching `StationSearchForm`/`CustomLineForm`'s existing fallback
  behavior — not a new gate to build.
- **No server-side real-station validation.** Design doc's Explicitly out
  of scope — a separate, unresolved question this plan doesn't touch.
- **No change to `useSuggestions.ts` or `suggestions.ts`.** Both are
  reused verbatim; the one harmless extra fetch on a pre-filled origin
  (design doc Decision 5) is accepted, not engineered around.
- **No `operator` case-normalization** (`.toUpperCase()`) to match
  `origin_crs`/`destination_crs`. Pre-existing asymmetry, not this plan's
  job (design doc Decision 3).
- **No Part 2 work** (trip/service search). Unrelated scope.

## Global Constraints

- **Testing:** `npm test` (`frontend/package.json`'s `"test": "vitest
  run"`) and `npm run build` (`next build`), both from `frontend/`.
- **File scope.** Modified: `frontend/components/TrackTrainForm.tsx`,
  `frontend/components/TrackTrainForm.test.tsx`. Nothing else.

---

### Task 1: Autocomplete the three fields, relax Origin validation — **frontend only**

**Files:**
- Modify: `frontend/components/TrackTrainForm.tsx`
- Modify: `frontend/components/TrackTrainForm.test.tsx`

- [ ] **Step 1: Add imports**

```tsx
import { Autocomplete } from '@mantine/core';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
```

Remove `TextInput` from the `@mantine/core` import list once all three
`TextInput` usages below are replaced (`DateTimePicker`'s row still needs
`Group`/`Button`, unaffected).

- [ ] **Step 2: Wire the three `useSuggestions` instances and the origin blur flag**

Directly below the existing `useState` block (`TrackTrainForm.tsx:52-58`):

```tsx
const { suggestions: originSuggestions } = useSuggestions(originCrs, searchStations);
const { suggestions: destinationSuggestions } = useSuggestions(destinationCrs, searchStations);
const { suggestions: operatorSuggestions } = useSuggestions(operator, searchTocs);
const [originTouched, setOriginTouched] = useState(false);
```

No new query-only state variables — per design doc Decision 1, the
existing `originCrs`/`destinationCrs`/`operator` state each double as
both the `Autocomplete`'s `value` and `useSuggestions`'s `query`,
matching `StationSearchForm.tsx`'s single-state shape, not
`CustomLineForm.tsx`'s query/value-split shape (that split only exists
there because its committed value is an array).

- [ ] **Step 3: Update `originValid`'s error gating**

`TrackTrainForm.tsx:60-61` keeps `originValid` computed exactly as today
(same regex, same expression) — do not change this line. Only the JSX
`error` prop (Step 4) changes to also require `originTouched`.

- [ ] **Step 4: Replace the Origin `TextInput` with `Autocomplete`**

Per design doc Decision 1's code sketch:

```tsx
<Autocomplete
  label="Origin CRS code"
  placeholder="e.g. Woking or WOK"
  value={originCrs}
  onChange={setOriginCrs}
  onBlur={() => setOriginTouched(true)}
  data={originSuggestions.map((s) => ({ value: s.code, label: s.code }))}
  filter={({ options }) => options}
  renderOption={({ option }) => {
    const match = originSuggestions.find((s) => s.code === option.value);
    return match ? `${match.code} — ${match.name}` : option.value;
  }}
  error={originTouched && originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
  required
/>
```

- [ ] **Step 5: Replace the Destination `TextInput` with `Autocomplete`**

Per design doc Decision 2's code sketch — same `data`/`filter`/
`renderOption` shape, fed by `destinationSuggestions`, no `error`, no
`required`, label text unchanged ("Destination CRS code (optional)").

- [ ] **Step 6: Replace the Operator `TextInput` with a single-value `Autocomplete`**

Per design doc Decision 3's code sketch — fed by `operatorSuggestions`
(from `searchTocs`), no `error`, no `required`, label text unchanged
("Operator (optional)"). Explicitly **not** a `TagsInput` — confirm the
final JSX has no `TagsInput` import or usage anywhere in this file.

- [ ] **Step 7: Update the one existing test whose assertion the blur-gate changes**

`TrackTrainForm.test.tsx:66-69` ("shows a field error for a non-3-letter
origin code") — add a `fireEvent.blur` on the origin field between the
existing `fireEvent.change` and the error assertion:

```tsx
it('shows a field error for a non-3-letter origin code', () => {
  renderWithMantine(<TrackTrainForm />);
  const field = screen.getByLabelText(/Origin CRS code/);
  fireEvent.change(field, { target: { value: 'WATERLOO' } });
  fireEvent.blur(field);
  expect(screen.getByText('Must be a 3-letter CRS code')).toBeInTheDocument();
});
```

- [ ] **Step 8: Add new test coverage for the autocomplete behavior**

New `it(...)` blocks in the same `describe('TrackTrainForm', ...)` block,
following this file's existing `vi.stubGlobal('fetch', ...)` pattern for
mocking the suggestion fetches (`searchStations`/`searchTocs` hit
`/api/stations`/`/api/tocs` via the same global `fetch`
`TrackTrainForm.test.tsx` already stubs for `/api/Train/track` — mock by
URL, mirroring the `attachTicketId` tests' `fetchMock.mockImplementation`
pattern at `:190-196` that branches on `String(input)`). Cover:

- Typing a partial origin query (e.g. `'Wok'`) does **not** show the
  `'Must be a 3-letter CRS code'` error while the field still has focus
  (no blur fired) — proves Decision 1's live-typing false-positive is
  actually gone, not just deferred.
- Blurring the origin field after typing a valid 3-letter code (e.g.
  `'WAT'`) shows no error — the blur gate doesn't turn a *valid* value
  into a false negative.
- Selecting a suggestion (simulate by firing `onChange` with the
  suggestion's code, since driving Mantine's real dropdown interaction
  needs `userEvent`, not `fireEvent` — check whether this file already
  imports `@testing-library/user-event`; if not, a direct `fireEvent.change`
  to the resolved code is an acceptable stand-in, consistent with how
  every other field in this file is already driven) still submits the
  correct `origin_crs` in the POST body.
- Leaving Destination/Operator empty still omits `destination_crs`/
  `operator` from the submitted JSON body (regression check for design
  doc Decision 2/3's "empty must submit as absent" point — assert on
  `JSON.parse(init!.body as string)` not having those keys, same pattern
  as the existing `service_date` assertion at `:273-275`).

- [ ] **Step 9: Test and build**

```bash
cd frontend
npm test
npm run build
```

Expected: all tests pass (including the updated Step 7 test and the new
Step 8 tests), `next build` succeeds with no new type errors.

- [ ] **Step 10: Commit**

```bash
git add frontend/components/TrackTrainForm.tsx frontend/components/TrackTrainForm.test.tsx
git commit -m "Add autocomplete to TrackTrainForm's origin, destination, and operator fields"
```

---

### Task 2: Final verification

- [ ] **Step 1: Full frontend verification**

```bash
cd frontend
npm test
npm run build
```

- [ ] **Step 2: Confirm no stray edits outside this plan's file scope**

```bash
git diff --stat main...HEAD
```

Compare against this plan's Global Constraints "File scope" list (exactly
two files) — flag anything unexpected before considering the branch done.

- [ ] **Step 3: Manual smoke check (if a running dev server is available)**

Load `/track`, type a partial station name into Origin (e.g. "Woking"),
confirm a dropdown of matching stations appears and no error flashes
while typing; confirm selecting one fills the field with the CRS code;
confirm Destination and Operator behave the same way and can be left
empty; confirm `/track?origin=WAT` still pre-fills Origin as before.

## Testing

Summarized (see Task 1 Step 7/8 for the authoritative detail):

- **`frontend`**: one existing `TrackTrainForm.test.tsx` assertion
  updated (blur-gated error, Step 7); new coverage for the no-live-error
  behavior, blur-with-valid-value, suggestion-selection-submits-correct-value,
  and empty-optional-fields-omit-keys-from-the-request-body (Step 8). No
  other test file in the repo references `TrackTrainForm` or its fields,
  so this is the complete testing surface for this change.
- **Backend**: no change, no new tests — `crates/` is untouched by this
  plan, per the design doc's confirmation that no backend change is
  needed.
- **CI**: runs under the existing frontend job unchanged — no new CI
  configuration needed.
