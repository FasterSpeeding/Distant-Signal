# Track a Train — Picker Refactor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.
>
> **Single task, frontend-only.** No backend change, no cross-task
> dependency — everything lands against one component file plus its
> colocated test file.

**Goal:** implement
`docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md`
end to end: move the picker to the end of the form, filter its rows by
Destination/Operator as well as Origin, retitle the Origin/Destination
labels, and make the picker container always visible with a distinct
state for every point before/after a fetch.

**Architecture:** zero new files, zero new shared infrastructure. Only
`frontend/components/TrackTrainForm.tsx` and
`frontend/components/TrackTrainForm.test.tsx` change.

**Design doc:**
`docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md`
— authoritative for every matching-semantics/copy/state detail below; this
plan does not repeat the reasoning, only the concrete steps.

---

## Non-goals

- **No backend query-string filtering.** Design doc's "Explicitly out of
  scope" — filtering is entirely client-side over already-fetched rows.
- **No resolve-via-suggestions filtering.** Design doc Decision 1 — the
  filter reads the field's literal value through `CRS_PATTERN`/
  `OPERATOR_PATTERN`, not a suggestion-resolved value.
- **No change to `pickDeparture`/`pickCifDeparture`, `TrackPinRequest`, or
  `POST /Train/track`.**
- **No loading spinner/skeleton component.** Plain `dimmed` `Text`, same
  visual weight as every other picker sentence.

## Global Constraints

- **Testing:** `cd frontend && npx tsc --noEmit`, `npm test`, `npm run
  build`.
- **File scope.** Modified: `frontend/components/TrackTrainForm.tsx`,
  `frontend/components/TrackTrainForm.test.tsx`. Nothing else.

---

### Task 1: Filtering helpers, `pickerLoading`, effect changes

**Files:** `frontend/components/TrackTrainForm.tsx`

- [ ] **Step 1: Add `OPERATOR_PATTERN` and the two matcher functions**

Directly below `CRS_PATTERN` (line 14), per design doc Decision 1's exact
code:

```tsx
const OPERATOR_PATTERN = /^[A-Za-z]{2}$/;

function matchesDestination(rowDestinationCrs: string | null, destinationCrs: string): boolean {
  const trimmed = destinationCrs.trim();
  if (!CRS_PATTERN.test(trimmed)) return true;
  return rowDestinationCrs !== null && rowDestinationCrs.toUpperCase() === trimmed.toUpperCase();
}

function matchesOperator(rowOperator: string, operator: string): boolean {
  const trimmed = operator.trim();
  if (!OPERATOR_PATTERN.test(trimmed)) return true;
  return rowOperator.toUpperCase() === trimmed.toUpperCase();
}
```

Include the doc comments from the design doc verbatim (they explain the
"no filtering while typing partial text" and CIF/`null` reasoning inline,
where a future reader will actually be looking).

- [ ] **Step 2: Add `pickerLoading` state**

Alongside the existing `const [picker, setPicker] = useState<Picker>(null);`:

```tsx
const [pickerLoading, setPickerLoading] = useState(() => CRS_PATTERN.test(initialOrigin.trim()));
```

- [ ] **Step 3: Wire `pickerLoading` into the existing fetch effect**

Per design doc Decision 5 — add `setPickerLoading(true)` right after the
`!originValid` early return's block, add `setPickerLoading(false)`
alongside each existing `setPicker(...)` call in the `.then` chain
(unavailable, non-ok-cif→null, cif-rows, non-ok-ldbws→null, ldbws-rows),
and guard the `.catch`:

```tsx
useEffect(() => {
  if (!originValid) {
    setPicker(null);
    setPickerLoading(false);
    return;
  }
  const controller = new AbortController();
  const crs = originCrs.trim().toUpperCase();
  setPickerLoading(true);

  fetch(`/api/stations/${crs}/departures`, { signal: controller.signal })
    .then((res) => {
      if (res.status === 404) {
        return fetch(`/api/stations/${crs}/schedule-departures`, { signal: controller.signal }).then(
          (cifRes) => {
            if (cifRes.status === 404) {
              setPickerLoading(false);
              return setPicker('unavailable');
            }
            if (!cifRes.ok) {
              setPickerLoading(false);
              return setPicker(null);
            }
            return cifRes.json().then((rows: ScheduleDepartureRow[]) => {
              setPickerLoading(false);
              setPicker({ source: 'cif', rows });
            });
          },
        );
      }
      if (!res.ok) {
        setPickerLoading(false);
        return setPicker(null);
      }
      return res.json().then((rows: DepartureRow[]) => {
        setPickerLoading(false);
        setPicker({ source: 'ldbws', rows });
      });
    })
    .catch(() => {
      if (!controller.signal.aborted) setPickerLoading(false);
    });
  return () => controller.abort();
}, [originCrs, originValid]);
```

- [ ] **Step 4: `tsc --noEmit` sanity check before moving to render changes**

```bash
cd frontend && npx tsc --noEmit
```

Expected clean (this step only adds state/functions, no render changes
yet, so nothing should break).

---

### Task 2: Relabel Origin/Destination, reorder the form, always-visible picker

**Files:** `frontend/components/TrackTrainForm.tsx`

- [ ] **Step 1: Relabel**

Origin `Autocomplete`'s `label` → `"Origin station"`. Destination
`Autocomplete`'s `label` → `"Destination station (optional)"`. No other
prop changes on either.

- [ ] **Step 2: Extract picker rendering into a `pickerContent()` function**

Replace the current four adjacent conditional JSX blocks (`:271-350`)
with one function, defined in the component body above the `return`,
implementing design doc Decision 4's six-state priority list exactly:

```tsx
function pickerContent() {
  if (!originValid) {
    return (
      <Text size="sm" c="dimmed">
        Enter an origin station above to see upcoming departures.
      </Text>
    );
  }
  if (pickerLoading) {
    return (
      <Text size="sm" c="dimmed">
        Checking for departures…
      </Text>
    );
  }
  if (picker === null) {
    return (
      <Text size="sm" c="dimmed">
        Couldn&apos;t load departures for this station right now — enter the details below.
      </Text>
    );
  }
  if (picker === 'unavailable') {
    return (
      <Text size="sm" c="dimmed">
        No departure information is available for this station — enter the details below.
      </Text>
    );
  }
  if (picker.rows.length === 0) {
    return (
      <Text size="sm" c="dimmed">
        No live departures currently on the board for this station right now.
      </Text>
    );
  }
  if (picker.source === 'ldbws') {
    const filtered = picker.rows.filter(
      (row) => matchesDestination(row.destinationCrs, destinationCrs) && matchesOperator(row.operator, operator),
    );
    if (filtered.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No upcoming departures match the destination and/or operator you&apos;ve entered.
        </Text>
      );
    }
    return (
      <ScrollArea mah={220} offsetScrollbars>
        <Stack gap="xs">
          {filtered.map((row) => {
            const clickable = !row.isCancelled;
            const badge = row.isCancelled ? (
              <Badge color="red">Cancelled</Badge>
            ) : row.delayMinutes > 0 ? (
              <Badge color="orange">+{row.delayMinutes} min</Badge>
            ) : (
              <Badge color="green">On time</Badge>
            );
            return (
              <Group
                key={row.serviceId}
                justify="space-between"
                wrap="nowrap"
                role={clickable ? 'button' : undefined}
                tabIndex={clickable ? 0 : undefined}
                onClick={clickable ? () => pickDeparture(row) : undefined}
                onKeyDown={
                  clickable
                    ? (event) => {
                        if (event.key === 'Enter' || event.key === ' ') pickDeparture(row);
                      }
                    : undefined
                }
                style={{ cursor: clickable ? 'pointer' : 'default', opacity: clickable ? 1 : 0.6 }}
              >
                <Text size="sm">
                  {row.scheduled} · {row.destinationCrs} · {row.operator}
                </Text>
                {badge}
              </Group>
            );
          })}
        </Stack>
      </ScrollArea>
    );
  }
  // picker.source === 'cif'
  const filtered = picker.rows.filter((row) => matchesDestination(row.destinationCrs, destinationCrs));
  return (
    <>
      <Text size="sm" c="dimmed">
        Live departure boards aren&apos;t available for this station. Showing the scheduled timetable instead —
        this is not live running information and may be up to 30 minutes out of date.
      </Text>
      {filtered.length === 0 ? (
        <Text size="sm" c="dimmed">
          No upcoming scheduled departures match the destination you&apos;ve entered.
        </Text>
      ) : (
        <ScrollArea mah={220} offsetScrollbars>
          <Stack gap="xs">
            {filtered.map((row) => (
              <Group
                key={row.uid}
                justify="space-between"
                wrap="nowrap"
                role="button"
                tabIndex={0}
                onClick={() => pickCifDeparture(row)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' || event.key === ' ') pickCifDeparture(row);
                }}
                style={{ cursor: 'pointer' }}
              >
                <Text size="sm">
                  {row.scheduled}
                  {row.destinationCrs ? ` · ${row.destinationCrs}` : ''}
                </Text>
              </Group>
            ))}
          </Stack>
        </ScrollArea>
      )}
    </>
  );
}
```

- [ ] **Step 3: Move the picker container to the end of the form, wrapped
  with a fixed `min-height`**

Reorder the JSX so the render sequence is: Origin → Scheduled departure
group → Destination → Operator → picker container → field-error `Alert`
→ submit button `Group`:

```tsx
<Stack gap="xs" mih={72}>
  {pickerContent()}
</Stack>
```

placed immediately after the Operator `Autocomplete` and immediately
before the `{fieldError && (...)}` block.

- [ ] **Step 4: `tsc --noEmit` and a quick visual sanity read**

```bash
cd frontend && npx tsc --noEmit
```

Re-read the whole returned JSX tree once to confirm: no leftover
duplicate picker-rendering blocks, no stray unused imports (none should
be needed — `Text`/`Badge`/`Group`/`ScrollArea`/`Stack` were already
imported), and the four fields render in the new documented order.

---

### Task 3: Update existing tests for the label/position/always-visible changes

**Files:** `frontend/components/TrackTrainForm.test.tsx`

- [ ] **Step 1: Global label-text updates**

Replace every `/Origin CRS code/` with `/Origin station/` and every
`/Destination CRS code/` with `/Destination station/` across the file
(mechanical — `getByRole`/`getByLabelText` name matchers only, no other
assertion changes needed at these call sites).

- [ ] **Step 2: Confirm the always-visible-container tests still pass
  under the new default-empty-Origin prompt state**

No existing test currently asserts on the pre-fetch/no-origin picker
region's *content* (the old code rendered nothing there) — nothing here
should break, since these tests already didn't assert the region was
absent. Re-run the suite (Task 4) to confirm no incidental collisions
between the new prompt sentence and existing `queryByText` assertions
elsewhere in the file.

- [ ] **Step 3: Run the suite once to find what actually breaks**

```bash
cd frontend && npm test
```

Fix any failures beyond the label-text ones already anticipated (e.g. if
a test happened to rely on DOM order between the picker and the other
fields — grep confirms none currently do; all locate elements by role/
label/text, not position).

---

### Task 4: New test coverage for filtering and the always-visible container

**Files:** `frontend/components/TrackTrainForm.test.tsx`

Add new `it(...)` blocks inside the existing `describe('live departures
picker', ...)` block, reusing its existing `mockFetchByUrl` helper and
`departures`/`scheduleDepartures` fixtures (extend fixtures only if an
existing row's `destinationCrs`/`operator` values don't already give a
new test something to filter on/off).

- [ ] **Step 1: Picker container present before Origin is valid**

```tsx
it('shows the picker container with a prompt before Origin is filled in', () => {
  renderWithMantine(<TrackTrainForm />);
  expect(
    screen.getByText('Enter an origin station above to see upcoming departures.'),
  ).toBeInTheDocument();
});
```

- [ ] **Step 2: Destination filters LDBWS rows once resolved**

Using the existing `departures` fixture (`svc-cancelled` → `WAT`,
`svc-on-time` → `BSK`): render with `initialOrigin="WAT"`, wait for the
departures fetch, set Destination to `'BSK'` (a resolved 3-letter code),
assert the `10:40` (BSK) row is still present/clickable and the `10:15`
(WAT) row's text is no longer in the document. Then, separately, assert
that setting Destination to a partial, non-3-letter value (e.g. `'Wo'`)
leaves both rows visible (no filtering yet).

- [ ] **Step 3: Operator filters LDBWS rows once resolved**

Same fixture: set Operator to `'SW'` (resolved 2-letter code), assert
only the `10:40`/`SW` row remains, the `10:15`/`ZA` row's text is gone.

- [ ] **Step 4: Operator filter does not eliminate CIF rows**

Using the existing `scheduleDepartures` fixture (no `operator` field at
all): render with the LDBWS→404, CIF→200 fallback path, set Operator to
some resolved 2-letter code (e.g. `'SW'`), assert both CIF rows
(`08:22`/`09:00`) are still present — proving Decision 1's CIF/Operator
exemption, not merely "wasn't tested."

- [ ] **Step 5: Destination filter can legitimately empty the CIF list**

Same CIF fixture: set Destination to a resolved code that matches
neither `'CRE'` nor the `null`-destination row (e.g. `'ZZZ'`), assert
the new "No upcoming scheduled departures match the destination you've
entered." text appears, and neither row's text remains.

- [ ] **Step 6: Filtered-to-zero LDBWS state shows its own sentence**

Using the existing `departures` fixture: set Destination to a resolved
code matching neither row (e.g. `'ZZZ'`), assert "No upcoming departures
match the destination and/or operator you've entered." appears and
neither `10:15` nor `10:40` remains.

- [ ] **Step 7: Run and fix**

```bash
cd frontend && npm test
```

---

### Task 5: Final verification and commit

- [ ] **Step 1: Full verification bar**

```bash
cd frontend
npx tsc --noEmit
npm test
npm run build
```

All three must be clean/passing.

- [ ] **Step 2: Confirm no stray edits outside file scope**

```bash
git diff --stat main...HEAD
```

Compare against Global Constraints' file scope (exactly two files besides
the two new docs).

- [ ] **Step 3: Re-read the diff against the four numbered requirements**

Manually confirm: (1) picker is last, filters by all fields, CIF/Operator
asymmetry handled without hiding the whole CIF list; (2) Origin/
Destination labels no longer say "CRS code"; (3) picker container is
always present with a sensible state in every case; (4) verification bar
passed.

- [ ] **Step 4: Commit**

Spec + plan docs as one commit, implementation as one or more separate
commits (e.g. one for Task 1+2's component change, one for Task 3+4's
test updates) — do not push, do not merge to main.

## Testing

- **`frontend`**: `TrackTrainForm.test.tsx` — every `Origin CRS code`/
  `Destination CRS code` label-text lookup updated (mechanical); new
  coverage for the always-visible prompt state and for Decision 1's
  filtering semantics (Destination/Operator narrowing LDBWS rows once
  resolved but not while partial, the CIF/Operator exemption, and both
  sources' "filtered to zero" sentences). No other test file references
  `TrackTrainForm` or its fields.
- **Backend**: no change, no new tests.
- **CI**: existing frontend job, unchanged configuration.
