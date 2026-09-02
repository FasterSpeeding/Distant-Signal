# Ticket Upload Drag-and-Drop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `TicketEntryForm`'s two plain Mantine `FileInput` upload
controls (the `.pkpass` and PDF tabs of `UploadPanel`,
`frontend/components/TicketEntryForm.tsx:385-423`) with `@mantine/dropzone`'s
`Dropzone` component, adding drag-and-drop selection to both tabs while
keeping click-to-browse working, with zero change to `handleUpload`'s
request/error-handling logic, `open`/`tab` state, or which tab a file can be
dropped on (each `Dropzone` stays scoped to its own tab's `kind`/`accept`,
exactly as its `FileInput` predecessor was).

**Architecture:**

```
frontend/package.json                  + "@mantine/dropzone": "9.5.2"
frontend/app/globals.css               + @import '@mantine/dropzone/styles.css'
        │                                (Task 1)
        ▼
frontend/components/TicketEntryForm.tsx
  UploadPanel: FileInput -> Dropzone     (Task 2)
  accept passed as an ARRAY, not the
  bare string FileInput took (Task 2,
  Step 1's finding -- load-bearing)
        │
        ├─────────────────────────────┐
        ▼                             ▼
  (Task 3: keyboard-activation   frontend/components/TicketEntryForm.test.tsx
   verification -- no source        getPkpassFileInput/getPdfFileInput kept,
   change expected, confirms        new createDtWithFiles-style helper +
   Task 2's props are correct)      drop-sourced test cases added (Task 4)
        │                             │
        └──────────────┬──────────────┘
                        ▼
              Task 5: vitest + tsc + build
              (+ manual keyboard/drop click-through)
```

**Tech Stack:** Next.js App Router (React 19.2, `'use client'` components) +
Mantine v9 (`@mantine/core`, `@mantine/dates`, `@mantine/charts`,
`@mantine/hooks` all exact-pinned at `9.5.2`) + `@mantine/dropzone@9.5.2`
(new, exact-pinned, peer-matches `react`/`react-dom` `^19.2.0` and
`@mantine/core`/`@mantine/hooks` `9.5.2` exactly — confirmed via `npm view
@mantine/dropzone@9.5.2 peerDependencies` this session, see Task 1) +
Vitest/`@testing-library/react` (existing convention, no new test
dependency — `react-dropzone@15.0.0`, `@mantine/dropzone`'s one transitive
dependency, ships its own well-established `fireEvent.drop`-based test
pattern this plan adapts, Task 4).

**Spec:**
`docs/superpowers/specs/2026-09-02-ticket-upload-drag-and-drop-design.md` —
read in full before starting; this plan does not restate its research, only
carries its Decisions into concrete tasks. Cross-references below to
"Decision N" refer to that document.

**Status note — every citation below re-confirmed directly against this
worktree's current source this session, not trusted blind from the spec:**
`frontend/components/TicketEntryForm.tsx`'s `UploadPanel` (lines 385-423),
its `FileInput` (402-407), the two `Tabs.Panel` call sites (`pkpass`:
351-354, `pdf`: 356-359), `handleUpload` (127-177), and the `@mantine/core`
import line (5) all match the spec's own citations exactly, confirmed by a
full read this session. `frontend/components/TicketEntryForm.test.tsx`'s
`getPkpassFileInput`/`getPdfFileInput` (42-50), their rationale comment
(28-41), the `it.each` status-code table (100-119), and `openForm` (23-26)
all match. `frontend/package.json`'s `dependencies` block (13-24) confirmed:
`@mantine/charts`/`@mantine/core`/`@mantine/dates`/`@mantine/hooks` all pin
`9.5.2` exactly, alphabetically ordered, no `@mantine/dropzone` entry yet.
`frontend/app/globals.css:1-2` confirmed: `@import '@mantine/core/styles.css';`
then `@import '@mantine/dates/styles.css';`, nothing else before the first
real rule. The three call sites the spec's "Corrections" section names were
independently re-confirmed by grep this session: `frontend/app/track/mine/page.tsx:113`
(`<TicketEntryForm label="Add a ticket" />`, no `trackingId`),
`frontend/components/TicketPanel.tsx:74` and `:98` (both with `trackingId`
set), and `frontend/app/track/page.tsx`'s only `TicketEntryForm` match is a
comment at line 14, confirming the spec's correction that `/track`
(`TrackTrainForm`) does not render this form.

**New findings this plan's own verification pass surfaced, not fully
resolved by the spec (both from reading the actual installed-version source
on unpkg/GitHub this session, not assumed):**

1. **`accept` must be passed to `Dropzone` as an array, not the bare string
   `FileInput` took.** `FileInput`'s `accept=".pkpass"` /
   `accept="application/pdf"` (plain strings) cannot be carried over
   unchanged. Tracing the actual code: Mantine's `Dropzone`
   (`@mantine/dropzone@9.5.2/esm/Dropzone.mjs`) only converts an **array**
   `accept` prop into the object shape `useDropzone` expects
   (`Array.isArray(accept) ? accept.reduce((r, key) => ({...r, [key]: []}), {}) : accept`)
   — a bare string passes through unconverted into `react-dropzone@15.0.0`'s
   `acceptPropAsAcceptAttr(accept)`, which calls `Object.entries(accept)` on
   it. `Object.entries` on a string indexes its characters (`Object.entries(".pkpass")`
   → `[["0","."],["1","p"],...]`), and none of those single-character
   "entries" pass the function's own `isMIMEType`/`isExt` filter, so the
   final `.join(",")` yields `""` — an **empty** `accept` HTML attribute on
   the real `<input type="file">`, not `.pkpass`/`application/pdf`. This
   would both defeat the native OS file-picker's own extension filtering and
   silently break `TicketEntryForm.test.tsx`'s existing
   `getPkpassFileInput`/`getPdfFileInput` helpers, which query
   `input[type="file"][accept=".pkpass"]` / `[accept="application/pdf"]`
   directly. The fix, verified against the same source: pass `accept` as an
   array — `accept={['.pkpass']}` for the pkpass tab (`.pkpass` has no
   registered MIME type, and `attr-accept`'s per-file matcher, confirmed
   separately below, already treats a leading-dot string as an extension
   match) and `accept={PDF_MIME_TYPE}` for the PDF tab (Mantine's own
   exported constant, confirmed via `@mantine/dropzone@9.5.2/esm/mime-types.mjs`
   to literally be `["application/pdf"]"`, i.e. already the correct shape).
   Task 2, Step 1 below is built around this finding.
2. **The dropped/selected file's *acceptance* (accept-vs-reject routing,
   Decision 3) does not depend on the above** — confirmed separately by
   reading `attr-accept@2.2.5`'s `accepts(file, acceptedFiles)`, the
   function `react-dropzone`'s `fileAccepted()` actually calls to decide
   `onDrop` vs `onReject`. It independently `.split(',')`s a plain string
   and matches a leading-dot entry against the filename's extension, so a
   bare string would have worked for *this* purpose — the array requirement
   above is specifically about the HTML `accept` attribute exposed on the
   rendered `<input>` (native dialog filtering + the existing test
   helpers), not about drop-acceptance routing itself.
3. **`Dropzone`'s default keyboard-accessibility is confirmed present**,
   resolving the spec's Open Question 1/Decision 5 flag — Task 3 below
   verifies this for real rather than trusting this citation blindly, but
   the citation is concrete: `@mantine/dropzone@9.5.2/esm/Dropzone.mjs`'s
   `defaultProps` sets `activateOnKeyboard: true`, which the component maps
   to `useDropzone({..., noKeyboard: !activateOnKeyboard})` — i.e.
   `noKeyboard: false` by default. `react-dropzone@15.0.0`'s own source
   (`dist/es/index.js`) shows that with `noKeyboard: false` and not
   `disabled`, `getRootProps()` includes `tabIndex: 0` and a `role`
   (`"presentation"` unless overridden) plus an `onKeyDown` handler that
   calls `event.preventDefault(); openFileDialog();` on Space/Enter
   (`event.key === ' ' || event.key === 'Enter' || event.keyCode === 32 ||
   event.keyCode === 13`) when the event target is the root itself.
   Mantine's `Dropzone` spreads `getRootProps()` onto its root `Box` with no
   subsequent override of `tabIndex`/`role`/keyboard handlers. This is
   independently corroborated by `react-dropzone`'s own test suite
   (`react-dropzone/src/index.spec.js` at the `v15.0.0` tag, confirmed via
   `curl` this session): `expect(container.querySelector("div")).toHaveAttribute("tabindex", "0")`
   by default, and `.not.toHaveAttribute("tabindex")` once `noKeyboard` is
   explicitly passed. No prop needs to be set explicitly — the default is
   already correct — but this is exactly the kind of claim the task
   instructions require a real runtime check for, not just a source read;
   Task 3 is that check.
4. **`@mantine/charts` (already a pinned dependency, used in
   `frontend/app/lines/[id]/history/TrendsCharts.tsx:3`) is a pre-existing
   counter-example to "every sibling `@mantine/*` package's `styles.css` is
   imported in `globals.css`"**: `@mantine/charts@9.5.2` does ship its own
   `styles.css`/`styles.layer.css` (confirmed via `unpkg` directory listing
   this session, 14.4 kB), but `frontend/app/globals.css:1-2` only imports
   `@mantine/core/styles.css` and `@mantine/dates/styles.css` — `@mantine/charts`'s
   stylesheet is never imported anywhere in this app, confirmed by grep.
   This is a **pre-existing gap in the app, unrelated to this plan** (charts
   render mostly as bare `recharts` SVG with inline styles, so the visual
   cost of the missing stylesheet is much smaller than it would be for
   `Dropzone`, whose default look — dashed border, icon layout, drag-state
   colors — depends structurally on its stylesheet) — **do not fix it as
   part of this plan**, and follow `@mantine/dates`'s import (the pattern
   that *is* actually followed today), not `@mantine/charts`'s, for Task 1.

## Global Constraints

- **No change to `handleUpload`'s signature, status-code branching, or any
  of its six response-status outcomes** (`TicketEntryForm.tsx:127-177`).
  Nothing about *how* a `File` reaches `handleUpload` — a `FileInput`
  `onChange` today, a `Dropzone` `onDrop` after this plan — is visible to
  that function. Decision 4. No task touches this function's body.
- **No change to `open`/`tab` state, the collapsed entry-point gating, or
  Mantine `Tabs`' always-mounted-panels behavior.** The form does not
  auto-open on a page-level drop and does not auto-switch tabs based on a
  dropped file's type — Decision 2. Only `UploadPanel`'s file-selection
  control itself changes.
- **No file-kind auto-detection.** Each `Dropzone` stays hard-scoped to its
  own tab's fixed `kind`/`accept` pair, exactly as its `FileInput`
  predecessor was (`kind="pkpass"` only ever calls `handleUpload(file,
  'pkpass')`; `kind="pdf"` only ever calls `handleUpload(file, 'pdf')`) —
  Decision 3. Do not add a unified/auto-routing drop target.
- **Single-file only.** `Dropzone` supports multi-file selection by default;
  this form's one-`FormData`-field-per-submission shape
  (`TicketEntryForm.tsx:133-134`) is unchanged, so every `Dropzone` instance
  must be constrained to one file (`multiple={false}`, and take only
  `files[0]` from `onDrop`'s callback array) — spec's "Explicitly out of
  scope: Multi-file drop/upload."
- **`FileInput` is not kept as a parallel fallback anywhere.** Decision 1/5:
  `Dropzone`'s built-in `activateOnClick` (default `true`) already covers
  the click-to-browse interaction `FileInput` provided; there is no "both
  controls, pick one" state to design. Remove the `FileInput` import from
  `TicketEntryForm.tsx` once both tabs are converted (Task 2), and do not
  reintroduce it as a manual keyboard-only fallback in Task 3 — Task 3 is a
  verification step, and per the design's own finding (this plan's
  "New findings" #3) no fallback should be needed.
- **`accept` is passed to each `Dropzone` as an array, not the bare string
  value its `FileInput` predecessor used** — this plan's "New findings" #1
  above is load-bearing for both the native file-picker's own filtering and
  the existing test helpers; Task 2 must not silently carry over
  `accept=".pkpass"` / `accept="application/pdf"` as plain strings.
- **`/track` (`TrackTrainForm`) is out of scope**, unchanged from the spec's
  own Corrections section: `TicketEntryForm` is not rendered there, so there
  is nothing to add drag-and-drop to on that page.
- **Testing convention:** colocated `*.test.tsx`, `@testing-library/react`,
  `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npm test` /
  `npx vitest run`, both run from `frontend/`). Every task below that
  touches `frontend/` code must leave `npx vitest run`, `npx tsc --noEmit`,
  and `npm run build` (all from `frontend/`) passing with no new failures —
  Task 5 is the final, full run of all three, but Tasks 2 and 4 each run
  the narrower, immediately-relevant subset before their own commit so a
  regression is caught at the task that introduced it, not deferred to the
  end.
- **Parallelizable tasks: none.** Task 2 depends on Task 1 (the dependency
  must exist to import `Dropzone`/`PDF_MIME_TYPE`). Task 3 depends on Task 2
  (there is nothing to verify keyboard-activation on until the component
  exists in the tree). Task 4 depends on Task 2 (the test file is rewritten
  against the new component's actual rendered DOM, confirmed empirically,
  not assumed) and benefits from Task 3 having already run (confirms the
  props Task 4's tests exercise are the final ones). Task 5 depends on all
  of the above. This is a small, single-file-focused change with no
  disjoint-file parallelism opportunity worth dispatching separately.

---

### Task 1: Add the `@mantine/dropzone` dependency and its stylesheet import

**Files:**
- Modify: `frontend/package.json:13-24`
- Modify: `frontend/app/globals.css:1-2`

**Interfaces:**
- Produces: the installed `@mantine/dropzone@9.5.2` package (importable as
  `import { Dropzone, PDF_MIME_TYPE } from '@mantine/dropzone';` from Task 2
  onward) and its stylesheet loaded app-wide.
- Consumed by: Task 2 (`Dropzone`/`PDF_MIME_TYPE` imports in
  `TicketEntryForm.tsx`).
- **Depends on:** nothing — this is the foundational task.

- [ ] **Step 1: Add the dependency to `package.json`**

In `frontend/package.json`'s `dependencies` block (currently lines 13-24,
alphabetically ordered), insert the new entry between `@mantine/dates` (line
16) and `@mantine/hooks` (line 17), exact-pinned matching every other
`@mantine/*` sibling in this file:

```json
    "@mantine/charts": "9.5.2",
    "@mantine/core": "9.5.2",
    "@mantine/dates": "9.5.2",
    "@mantine/dropzone": "9.5.2",
    "@mantine/hooks": "9.5.2",
```

- [ ] **Step 2: Install and verify the lockfile**

Run (from `frontend/`): `npm install`

Expected: `frontend/package-lock.json` gains a `@mantine/dropzone@9.5.2`
entry and a new transitive `react-dropzone@15.0.0` entry (confirmed via `npm
view @mantine/dropzone@9.5.2 dependencies` this session to be
`@mantine/dropzone`'s only dependency). No `--force`/`--legacy-peer-deps`
flag should be needed — the peer bounds (`react`/`react-dom` `^19.2.0`,
`@mantine/core`/`@mantine/hooks` `9.5.2`) are exact matches for what's
already installed, confirmed via `npm view @mantine/dropzone@9.5.2
peerDependencies` this session. If `npm install` reports any peer conflict,
stop and re-verify the installed `@mantine/core`/`@mantine/hooks` versions
before forcing anything — that would mean this plan's own version-match
citation is stale.

- [ ] **Step 3: Import the stylesheet**

In `frontend/app/globals.css`, immediately after the existing
`@mantine/dates/styles.css` import (line 2), following the same pattern
already used for `@mantine/core` and `@mantine/dates` (not `@mantine/charts`
— see this plan's "New findings" #4 for why `@mantine/charts`'s own missing
stylesheet import is a separate, pre-existing gap this task does not touch):

```css
@import '@mantine/core/styles.css';
@import '@mantine/dates/styles.css';
@import '@mantine/dropzone/styles.css';
```

- [ ] **Step 4: Sanity-check the app still builds with the new dependency present but unused**

Run (from `frontend/`): `npx tsc --noEmit && npm run build`

Expected: PASS, identical to before this task (nothing yet imports
`@mantine/dropzone` in application code — this step only confirms the
dependency install and stylesheet import don't themselves break anything
before Task 2 starts using the package).

- [ ] **Step 5: Commit**

```bash
git add frontend/package.json frontend/package-lock.json frontend/app/globals.css
git commit -m "Add @mantine/dropzone 9.5.2 dependency and its stylesheet import"
```

---

### Task 2: Replace `UploadPanel`'s `FileInput` with `Dropzone` on both tabs

**Files:**
- Modify: `frontend/components/TicketEntryForm.tsx:1-9` (imports)
- Modify: `frontend/components/TicketEntryForm.tsx:385-423` (`UploadPanel`)

**Interfaces:**
- Consumes: `Dropzone`, `PDF_MIME_TYPE` from `@mantine/dropzone` (Task 1).
- Produces: `UploadPanel`'s external contract (`kind`, `accept`, `uploading`,
  `error`, `onFile`, `onFallback` props) stays unchanged — no caller outside
  this file needs to change (the two `<UploadPanel kind=... accept=...
  ...>` call sites at `TicketEntryForm.tsx:352-353` and `:357-358` are
  untouched). `accept`'s *value* at each call site changes shape (string ->
  array, this plan's "New findings" #1), which is why this task also touches
  those two call sites, not just `UploadPanel`'s internals.
- **Depends on:** Task 1.

- [ ] **Step 1: Change the `accept` prop's type and the two call sites' values**

`UploadPanel`'s prop type (currently `accept: string;`,
`TicketEntryForm.tsx:394`) becomes `accept: string[];`. Update the two
`Tabs.Panel` call sites (`TicketEntryForm.tsx:351-354` and `:356-359`) to
pass arrays instead of bare strings — per this plan's "New findings" #1,
passing the old bare-string values through unchanged would silently zero
out the rendered `<input accept="">` HTML attribute:

```tsx
        <Tabs.Panel value="pkpass" pt="md">
          <UploadPanel kind="pkpass" accept={['.pkpass']} uploading={uploading} error={uploadError} onFile={handleUpload}
            onFallback={() => setTab('manual')} />
        </Tabs.Panel>

        <Tabs.Panel value="pdf" pt="md">
          <UploadPanel kind="pdf" accept={PDF_MIME_TYPE} uploading={uploading} error={uploadError} onFile={handleUpload}
            onFallback={() => setTab('manual')} />
        </Tabs.Panel>
```

`PDF_MIME_TYPE` (imported from `@mantine/dropzone` in Step 2 below) is
Mantine's own exported constant, confirmed this session
(`@mantine/dropzone@9.5.2/esm/mime-types.mjs`) to literally equal
`["application/pdf"]` — the same value this task would otherwise have
hand-written as `['application/pdf']`, so using the named constant is a
direct, non-behavior-changing substitution.

- [ ] **Step 2: Update the import line**

`TicketEntryForm.tsx:5` currently:

```tsx
import { Alert, Button, FileInput, Group, Stack, Tabs, TextInput, Text } from '@mantine/core';
```

Remove `FileInput` (no longer used anywhere in this file after Step 3) and
add the new import:

```tsx
import { Alert, Button, Group, Stack, Tabs, TextInput, Text } from '@mantine/core';
import { Dropzone, PDF_MIME_TYPE } from '@mantine/dropzone';
```

- [ ] **Step 3: Replace `UploadPanel`'s `FileInput` with `Dropzone`**

Current body (`TicketEntryForm.tsx:400-407`):

```tsx
  return (
    <Stack gap="sm">
      <FileInput
        label={kind === 'pkpass' ? 'Apple Wallet .pkpass file' : 'PDF e-ticket'}
        accept={accept}
        disabled={uploading}
        onChange={(file) => onFile(file, kind)}
      />
```

Replace with:

```tsx
  return (
    <Stack gap="sm">
      <Dropzone
        accept={accept}
        multiple={false}
        loading={uploading}
        onDrop={(files) => onFile(files[0] ?? null, kind)}
        onReject={() => {
          /* Decision 3: a mismatched file type is a client-side pre-filter,
           * not a request -- no request reaches handleUpload, so there is
           * nothing to report through the existing uploadError/UploadPanel
           * error path. Rendering polish (an inline "wrong file type"
           * message via Dropzone.Reject) is explicitly left to the
           * implementer per the spec's "Explicitly out of scope: Visual
           * design of the drop area itself." */
        }}
      >
        <Stack gap={4} align="center" style={{ pointerEvents: 'none' }}>
          <Text size="sm" fw={500}>
            {kind === 'pkpass' ? 'Apple Wallet .pkpass file' : 'PDF e-ticket'}
          </Text>
          <Text size="xs" c="dimmed">
            Drag and drop, or click to browse
          </Text>
        </Stack>
      </Dropzone>
```

This keeps `UploadPanel`'s public contract identical (same `onFile(file,
kind)` call shape as before — `files[0] ?? null` maps `Dropzone`'s
`onDrop(files: File[])` back onto the same `File | null` `handleUpload`
already accepts, per Decision 4's "zero changes to `handleUpload`"). The
visible copy ("Apple Wallet .pkpass file" / "PDF e-ticket") is carried over
verbatim from `FileInput`'s old `label` prop, so this is a control swap, not
a copy change. `style={{ pointerEvents: 'none' }}` on the inner content is
the same pattern Mantine's own `Dropzone` examples use so a click anywhere
inside the drop area (not just on the text) reaches the dropzone's own click
handler rather than being intercepted by a child element — verify this is
still necessary/correctly placed once the component renders (Step 5 below),
since it's a rendering-polish detail the spec explicitly left to
implementation, not a data-flow requirement this plan is prescriptive about.

The `error` Alert block below (`TicketEntryForm.tsx:408-420`) is untouched —
Decision 4, error handling is unchanged.

- [ ] **Step 4: Confirm `Dropzone`'s `disabled` state during upload**

Mantine's `Dropzone` passes `disabled: disabled || loading` into
`useDropzone` internally (confirmed via
`@mantine/dropzone@9.5.2/esm/Dropzone.mjs` this session) — so `loading=
{uploading}` in Step 3 already disables drag/click/keyboard activation while
an upload is in flight, matching `FileInput`'s old `disabled={uploading}`
behavior with no separate `disabled` prop needed. Do not add a redundant
`disabled={uploading}` prop alongside `loading={uploading}`.

- [ ] **Step 5: Run the app locally and eyeball both tabs**

Run (from `frontend/`): `npm run dev`, navigate to `/track/mine`, click "Add
a ticket", switch to the "Upload .pkpass" and "Upload PDF e-ticket" tabs.
Confirm: each tab shows a drop area (not a blank/broken control), dragging a
file over it shows Mantine's drag-state styling, and clicking it opens the
native file picker. Fix any obvious visual issue (e.g. the `pointerEvents`
placement from Step 3) before moving on — this is a quick manual check, not
a substitute for Task 5's fuller verification pass.

- [ ] **Step 6: Run the existing test suite (expect failures — do not fix yet)**

Run (from `frontend/`): `npx vitest run TicketEntryForm`

Expected: the manual-entry and submit tests still PASS (they never touch
file selection), but every upload-path test (`it.each` at
`TicketEntryForm.test.tsx:100-119`, the 200-response tests, the standalone
pkpass-upload test) now FAILs, because `fireEvent.change(getPkpassFileInput(),
...)` no longer reaches a real `<input type="file">` the same way — this is
expected and is exactly what Task 4 fixes. Do not attempt to make these
pass in this task; confirming the *type* of failure (not e.g. a crash or an
import error) is this step's only purpose, so Task 4 starts from a known,
understood state rather than a surprise.

- [ ] **Step 7: `tsc` check**

Run (from `frontend/`): `npx tsc --noEmit`

Expected: PASS. This catches the `accept: string[]` type-signature change
(Step 1) and the removed `FileInput` import (Step 2) compiling cleanly, even
though the test file itself isn't fixed until Task 4.

- [ ] **Step 8: Commit**

```bash
git add frontend/components/TicketEntryForm.tsx
git commit -m "Replace TicketEntryForm's FileInput upload controls with @mantine/dropzone's Dropzone"
```

---

### Task 3: Verify keyboard-activation behavior for real

**Files:** none modified (verification-only task; a source change is only
made if the verification in Step 2 below fails, in which case Step 3
applies the fix inline in this task rather than deferring it).

**Interfaces:** none new.

- **Depends on:** Task 2 (the `Dropzone` must exist in the tree to verify
  against).

This task exists because the design spec explicitly flags `Dropzone`'s
default keyboard-accessibility as "not independently confirmed" (Decision 5,
Open Question 1) and this plan's own "New findings" #3 resolves it only at
the *source-reading* level (confirmed via `@mantine/dropzone@9.5.2` and
`react-dropzone@15.0.0`'s actual shipped code, and corroborated by
`react-dropzone`'s own test-suite assertion that `tabIndex="0"` is the
default) — not yet against this app's actual running build. The task
instructions this plan was written from are explicit that this must be
checked for real, not just assumed from the source read.

- [ ] **Step 1: Manual keyboard-only interaction check**

With `npm run dev` running (from Task 2, Step 5) and a mouse not used for
this check: navigate to `/track/mine` with the keyboard (Tab through the
page), reach the "Add a ticket" button via Tab, press Enter/Space to open
the form, Tab to the "Upload .pkpass" tab, press Enter/Space (or an arrow
key, per Mantine `Tabs`' own existing keyboard handling — unrelated to this
plan) to select it, then Tab again until focus visibly lands on the
`Dropzone` drop area itself (confirm a visible focus ring/outline appears on
it — if none is visible, that's a separate, real accessibility gap worth
noting even if focus/activation technically works). With the `Dropzone`
focused, press Enter, then separately (a fresh reload) press Space, and
confirm each opens the native file-picker dialog.

Expected, per this plan's "New findings" #3: both Enter and Space open the
file dialog, matching `react-dropzone`'s default `noKeyboard: false`
behavior. If this does **not** happen (dialog doesn't open, or the
`Dropzone` never receives visible focus via Tab at all), treat this as a
real bug in the finding above, not a rendering nitpick — proceed to Step 2.

- [ ] **Step 2: If Step 1 fails, inspect the actual rendered DOM**

Open the browser devtools, inspect the `Dropzone` root element in the "Upload
.pkpass" tab, and confirm/deny: does it have `tabindex="0"`? Does a
`keydown` listener fire on Enter/Space (check via the Elements panel's
Event Listeners tab, or a temporary `console.log` in a local, uncommitted
edit)? This narrows whether the gap is in `@mantine/dropzone`'s actual
shipped build (possible drift from the `unpkg`-read source this plan's
research relied on) versus something in this app's own build pipeline
(e.g. Next.js/webpack stripping the attribute, an unrelated global CSS rule
setting `pointer-events` or `outline: none` app-wide that masks focus
visibility without breaking actual activation).

- [ ] **Step 3: If Step 1 fails, apply the fix**

If the root cause is `activateOnKeyboard` not actually defaulting to `true`
in the installed build (contradicting this plan's source-read finding),
explicitly set `activateOnKeyboard` on both `Dropzone` instances in
`frontend/components/TicketEntryForm.tsx` (Task 2's `UploadPanel`):

```tsx
      <Dropzone
        accept={accept}
        multiple={false}
        loading={uploading}
        activateOnKeyboard
        onDrop={(files) => onFile(files[0] ?? null, kind)}
        onReject={() => {}}
      >
```

If the root cause is instead an app-level style issue (e.g. missing focus
outline), fix that narrowly (e.g. confirm `@mantine/dropzone/styles.css`
from Task 1 is actually being loaded — check the Network tab — rather than
adding new bespoke CSS) rather than reaching for `activateOnKeyboard`, which
would be the wrong fix for a purely visual gap. Whatever the fix, re-run
Step 1's manual check afterward to confirm it resolves the issue before
moving on.

- [ ] **Step 4: Record the outcome**

If Step 1 passed without any change: no commit needed for this task — the
verification confirms Task 2's props are already correct, and this plan's
"New findings" #3 stands confirmed both by source and by a live check. If
Step 3 applied a fix, commit it:

```bash
git add frontend/components/TicketEntryForm.tsx
git commit -m "Explicitly enable Dropzone keyboard activation (verified default was insufficient in this app's build)"
```

(Only run this commit step if Step 3 actually changed a file — do not
create an empty commit.)

---

### Task 4: Update `TicketEntryForm.test.tsx` for the new `Dropzone` control

**Files:**
- Modify: `frontend/components/TicketEntryForm.test.tsx`

**Interfaces:**
- Consumes: the rendered DOM `Dropzone` produces (Task 2) — specifically,
  its hidden `<input type="file" accept="...">` for the click-to-browse
  path (should be unchanged in query shape after Task 2 Step 1's `accept`
  array fix, but confirmed empirically in Step 1 below rather than assumed)
  and its focusable root element (`tabindex="0"`, confirmed in Task 3) for
  the new drop-sourced tests.
- Produces: no new exported interface — this task only extends the existing
  test file.
- **Depends on:** Task 2 (tests are written against the real rendered DOM,
  not guessed). Benefits from Task 3 already being done (confirms the props
  under test are final), but does not strictly require it — Task 3 finding
  no bug would mean no prop changes land after this task's tests are
  written; if Task 3 does apply a fix, re-run this task's tests afterward as
  part of Task 5's final pass regardless.

- [ ] **Step 1: Confirm the existing `getPkpassFileInput`/`getPdfFileInput` helpers still resolve correctly**

With Task 2 applied, run a scratch check — open the running dev server
(`npm run dev`) or a temporary `console.log(document.querySelector('input[type="file"][accept=".pkpass"]'))`
inside a quick, throwaway test — and confirm the hidden `<input
type="file">` react-dropzone renders for its click-to-browse path still
carries `accept=".pkpass"` / `accept="application/pdf"` literally, matching
`getPkpassFileInput`/`getPdfFileInput`'s existing query
(`TicketEntryForm.test.tsx:42-50`). Per this plan's "New findings" #1, this
should now hold precisely *because* Task 2 passes `accept` as an array
(`['.pkpass']` / `PDF_MIME_TYPE`, i.e. `["application/pdf"]`) rather than a
bare string — `acceptPropAsAcceptAttr`'s `Object.entries` over an
already-object-shaped `{'.pkpass': []}` (Mantine's own array-to-object
conversion) correctly reduces to the literal string `".pkpass"` per this
session's source read. If this check contradicts that expectation, do not
proceed to Step 2 as if the helpers are unchanged — update
`getPkpassFileInput`/`getPdfFileInput`'s query first to match whatever the
real rendered `accept` attribute value turns out to be, then continue.

Expected: the helpers keep working unmodified — no code change in this
step, verification only.

- [ ] **Step 2: Add the drop-event simulation helper**

`react-dropzone@15.0.0`'s own test suite (`src/index.spec.js`, confirmed via
direct fetch of the `v15.0.0` tag this session) uses exactly this shape to
build a fake `DataTransfer` for `fireEvent.drop`/`fireEvent.dragEnter`; add
an equivalent near the top of `TicketEntryForm.test.tsx`, alongside
`getPkpassFileInput`/`getPdfFileInput`:

```tsx
  // Mirrors react-dropzone's own test helper (react-dropzone/src/index.spec.js,
  // createDtWithFiles) -- the shape react-dropzone@15.0.0's internal
  // onDrop/onDragEnter handlers actually read off a native DragEvent's
  // dataTransfer, confirmed against that file directly rather than guessed.
  function dropFiles(node: Element, files: File[]) {
    const dataTransfer = {
      files,
      items: files.map((file) => ({
        kind: 'file',
        size: file.size,
        type: file.type,
        getAsFile: () => file,
      })),
      types: ['Files'],
    };
    fireEvent.drop(node, { dataTransfer });
  }

  // The Dropzone's own focusable/drop-target root is an ancestor of its
  // hidden file input (react-dropzone renders `<input {...getInputProps()}
  // />` as a child of the `<div {...getRootProps()}>` element Mantine's
  // Dropzone renders) -- reuse the existing accept-based input queries to
  // locate it reliably, rather than adding a new, separate selector.
  function getPkpassDropzoneRoot(): HTMLElement {
    return getPkpassFileInput().closest('[tabindex]') as HTMLElement;
  }

  function getPdfDropzoneRoot(): HTMLElement {
    return getPdfFileInput().closest('[tabindex]') as HTMLElement;
  }
```

Confirm `closest('[tabindex]')` actually resolves to the `Dropzone` root
(not some unrelated ancestor) by a quick manual check against Task 2's
rendered DOM before relying on it across every new test below — if the
`tabindex` attribute lives on a different element than expected, adjust the
selector (e.g. a specific class-name substring Mantine's `Dropzone` applies)
rather than guessing further.

- [ ] **Step 3: Rewrite the existing `it.each` upload-status table to use `dropFiles`**

Current (`TicketEntryForm.test.tsx:100-119`) drives every status-code case
via `fireEvent.change(getPkpassFileInput(), ...)`. Per this task's own scope
(Task 4's brief: cover both drop-sourced and click-sourced paths, not
replace one with the other), **keep** the existing `fireEvent.change`-based
version of this table for the click-to-browse path (Step 1 confirms it
still works), and add a **second**, drop-sourced version alongside it
rather than replacing it in place — this directly matches the spec's
Testing section: "Every existing `it.each` status-code case
... stays green unmodified for the click-to-browse path ... this addition
should not need to touch or duplicate that table, only add drop-sourced
variants alongside it":

```tsx
  it.each([
    [400, "That doesn't look like a valid upload — try again or fill in the form manually"],
    [422, 'could not read this as a train .pkpass: not a zip file'],
    [504, 'That file took too long to read — try a smaller or simpler PDF, or fill in the details manually'],
    [413, 'That file is too large (8 MB limit). Try filling in the details manually'],
    [500, "Couldn't read this file. Try filling in the details manually"],
  ])('pkpass drop: a %i response shows the mapped inline message', async (status, expectedSubstring) => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(status === 422 ? 'could not read this as a train .pkpass: not a zip file' : 'error', { status }),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    dropFiles(getPkpassDropzoneRoot(), [file]);

    expect(await screen.findByText(expectedSubstring)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'or fill in the details manually' })).toBeInTheDocument();
  });
```

- [ ] **Step 4: Add the drop-sourced success-path test (pkpass)**

```tsx
  it('pkpass drop: on a 200, pre-fills manual fields and switches to the manual tab', async () => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(
        JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: 'KGX', destinationCrs: null, source: 'pkpass-heuristic' }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    dropFiles(getPkpassDropzoneRoot(), [file]);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Manual entry', selected: true })).toBeInTheDocument();
    });
    expect(screen.getByLabelText('Operator')).toHaveValue('LNER');
    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/1/tickets/pkpass', expect.objectContaining({ method: 'POST' }));
    });
  });
```

- [ ] **Step 5: Add the drop-sourced success-path test (pdf)**

```tsx
  it('pdf drop: posts to the pdf-specific upload route, not the pkpass one', async () => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(
        JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: null, destinationCrs: null, source: 'pdf-heuristic' }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload PDF e-ticket' }));
    const file = new File(['fake'], 'ticket.pdf', { type: 'application/pdf' });
    dropFiles(getPdfDropzoneRoot(), [file]);

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/1/tickets/pdf', expect.objectContaining({ method: 'POST' }));
    });
  });
```

- [ ] **Step 6: Add the mismatched-file-type rejection test**

Per Decision 3/the spec's Testing section: a file that doesn't match a
tab's `accept` must not call `fetch` at all.

```tsx
  it('dropping a mismatched file type on the pkpass tab does not call fetch', async () => {
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['%PDF-1.4'], 'ticket.pdf', { type: 'application/pdf' });
    dropFiles(getPkpassDropzoneRoot(), [file]);

    // Give any (incorrect) async path a chance to run before asserting a
    // negative -- consistent with this file's existing style of asserting
    // absence via waitFor's polling rather than a bare synchronous check.
    await waitFor(() => expect(fetch).not.toHaveBeenCalled());
  });
```

- [ ] **Step 7: Run the full test file**

Run (from `frontend/`): `npx vitest run TicketEntryForm`

Expected: PASS — every original test (manual entry, submit, standalone
ticket, the original click-sourced `it.each` table) plus every new
drop-sourced test from Steps 3-6. If the drop-sourced tests fail while the
click-sourced ones pass, revisit Step 2's `dropFiles`/`getPkpassDropzoneRoot`
helpers against the actual DOM (per Step 2's own instruction to confirm,
not assume) before changing any test's assertions.

- [ ] **Step 8: Commit**

```bash
git add frontend/components/TicketEntryForm.test.tsx
git commit -m "Add drop-sourced test coverage for TicketEntryForm's Dropzone upload controls"
```

---

### Task 5: Final verification

**Files:** none modified (verification-only task).

**Interfaces:** none new.

- **Depends on:** Tasks 1-4.

- [ ] **Step 1: Full automated verification**

Run, from `frontend/`, in this order (stop and fix at the first failure
before continuing — do not skip ahead):

```bash
npx vitest run
npx tsc --noEmit
npm run build
```

Expected: all three PASS with zero new failures anywhere in the suite (not
just `TicketEntryForm.test.tsx` — confirm no other test file references
`FileInput`/`getByLabelText` against this form in a way Task 2's removal of
`FileInput` could have broken; a repo-wide grep for `TicketEntryForm` inside
`*.test.tsx` files other than its own colocated one is a quick way to check
this, and per the spec's own confirmed call-site list, none exist beyond
`TicketEntryForm.test.tsx` itself, `TicketPanel`'s own tests if any exist,
and each page's own tests if they render `TicketPanel`/`TicketEntryForm` —
worth a final `grep -rln "TicketEntryForm\|TicketPanel" frontend --include=*.test.tsx`
pass here to be certain).

- [ ] **Step 2: Manual drag-and-drop click-through**

With `npm run dev` running, on `/track/mine`: open the form, drag a real
`.pkpass`-named file from the OS file manager onto the "Upload .pkpass"
tab's drop area (a fake/empty file is fine — this is checking the UI
interaction, not upload correctness, which the automated tests already
cover) and confirm the drag-state styling appears during the drag and the
upload request fires on drop. Repeat for the PDF tab. Then repeat Task 3's
keyboard-only check once more end-to-end (Tab to the tab, Tab to the
Dropzone, Enter to open the native picker) to catch any regression Task 4's
test-writing might have introduced via an unrelated prop change.

- [ ] **Step 3: Confirm no stray FileInput usage remains**

Run: `grep -n "FileInput" frontend/components/TicketEntryForm.tsx`

Expected: no matches (Task 2, Step 2 removed the import; Task 2, Step 3
removed the two usages). If any match remains, Task 2 was incomplete —
finish removing it before considering this plan done.

- [ ] **Step 4: Final commit (only if Steps 1-3 surfaced fixes)**

If Step 1's build/test/typecheck pass or Steps 2-3's manual checks pass with
no changes needed, no commit is required for this task — it's pure
verification of work already committed in Tasks 1-4. If any step required a
fix, commit that fix with a message describing what the verification pass
caught (e.g. `git commit -m "Fix <specific issue> found during final
verification"`), then re-run Step 1 in full before considering the plan
complete.
