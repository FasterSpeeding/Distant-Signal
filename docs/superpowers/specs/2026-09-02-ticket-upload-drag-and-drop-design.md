# Design: Drag-and-Drop for Ticket Upload

**Status: design proposal, not approved.** Design only — no code changes,
no dependency added, no new file besides this spec. Written to the same
rigor as `docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md`.

## Goal

`TicketEntryForm` (`frontend/components/TicketEntryForm.tsx`) already lets a
user upload a `.pkpass` or PDF e-ticket, but only via a plain Mantine
`FileInput` — a click-to-open-file-browser control, no drag-and-drop. This
spec designs adding drag-and-drop file selection to the two upload tabs,
across every place this form is actually reachable, without changing any of
the existing upload/submit/error-handling logic behind it.

## Corrections to the brief

One premise in this task's brief doesn't hold up against the code: **`/track`
(`frontend/app/track/page.tsx`) does not render `TicketEntryForm`.** Read in
full, that page renders only `<TrackTrainForm .../>`
(`frontend/app/track/page.tsx:30`); `TrackTrainForm.tsx` itself has no
`TicketEntryForm` import or usage either — confirmed by reading it in full
and by `grep -rn "TicketEntryForm" frontend` (below), which shows
`app/track/page.tsx`'s only match is a comment (line 14) referring to
`TicketEntryForm`'s own "find or track the train" link that sends a user
*to* `/track`, not a render of the form itself. The confusion is
understandable — `TicketEntryForm`'s own doc comment
(`TicketEntryForm.tsx:14-16`) and `TrackTrainForm`'s doc comment
(`TrackTrainForm.tsx:20-21`) both reference each other, since a saved
standalone ticket's "next step" link (`TicketEntryForm.tsx:256`) navigates to
`/track?origin=...&ticketId=...`, and `/track`'s own page reads that
`ticketId` param (`app/track/page.tsx:16-20`) to pass `attachTicketId` into
`TrackTrainForm`. That's a one-way handoff between two different forms, not
an embedding.

The real, exhaustive set of call sites (`grep -rn "TicketEntryForm"
frontend`, cross-checked against each file's own render, confirmed this
session):

1. `frontend/app/track/mine/page.tsx:113` — `<TicketEntryForm label="Add a
   ticket" />`, no `trackingId` (a **standalone** ticket).
2. `frontend/components/TicketPanel.tsx:74` — `<TicketEntryForm
   trackingId={trackingId} label="Add a ticket for this journey" />`, shown
   when a tracked train has zero tickets yet.
3. `frontend/components/TicketPanel.tsx:98` — `<TicketEntryForm
   trackingId={trackingId} label="Add another ticket" />`, shown after the
   existing-tickets list, when at least one ticket already exists.

`TicketPanel` itself is embedded on both individual tracked-train detail
pages: `frontend/app/train/by-id/[trackingId]/page.tsx:57` and
`frontend/app/train/[uid]/[date]/page.tsx:58` (confirmed by reading both;
`TicketPanel.tsx:8-9`'s own doc comment states the same "renders on both"
claim, matched by the actual imports/renders in each page). So
`TicketEntryForm` is reachable from exactly three page shapes: `/track/mine`,
`/train/by-id/[trackingId]`, and `/train/[uid]/[date]` — never from `/track`.
This spec designs for all three, unchanged from the brief's intent (the
"create train/ticket page" language in the brief maps onto these three, not
`/track`).

## Current relevant state (verified this session)

- **`TicketEntryForm.tsx:283-360`**: a `Tabs` with three panels — `manual`
  (always mounted, default), `pkpass`, `pdf`. Both upload panels render the
  same `UploadPanel` helper (`TicketEntryForm.tsx:385-423`) with a different
  `kind`/`accept` pair: `kind="pkpass" accept=".pkpass"`
  (`TicketEntryForm.tsx:352-353`) and `kind="pdf" accept="application/pdf"`
  (`TicketEntryForm.tsx:357-358`).
- **`UploadPanel`'s file control** (`TicketEntryForm.tsx:402-407`): a bare
  Mantine `<FileInput label=... accept={accept} disabled={uploading}
  onChange={(file) => onFile(file, kind)} />`. `onFile` is `handleUpload`
  (`TicketEntryForm.tsx:127`), called with a fixed `kind` baked into each
  `UploadPanel` instance — the component never has to infer file type from
  content, only from which tab's control fired.
- **`handleUpload(file, kind)`** (`TicketEntryForm.tsx:127-177`): builds a
  `FormData`, `POST`s to `/api/${ticketsBasePath}/${kind}`, and branches on
  response status: `200` → `applyPreview` (also force-switches to the manual
  tab, `TicketEntryForm.tsx:124`); `401` → `needsLogin`; `400` → fixed
  message; `422` → backend's own text surfaced verbatim; `504` → fixed
  "took too long" message; `413` → fixed "too large (8 MB limit)" message;
  anything else (including a thrown/network error) → fixed generic message.
  None of this branches on *how* `file` was obtained — it only ever sees a
  `File`.
- **Collapse/entry-point gating** (`TicketEntryForm.tsx:275-281`): the whole
  form is gated behind `open` state, starting `false`; the only way in is
  clicking the `label` button. While collapsed, none of the `Tabs`/`FileInput`
  DOM exists at all.
- **Tab switching resets error state, not tab-specific content**
  (`TicketEntryForm.tsx:286-294`): `onChange` on `Tabs` clears `uploadError`
  only. Mantine's `Tabs` keeps every panel mounted (`display: none` on the
  inactive ones, per `TicketEntryForm.test.tsx:39-41`'s own comment,
  confirmed this session) — both hidden `<input type="file">` elements exist
  in the DOM simultaneously regardless of which tab is visually active.
- **`frontend/package.json`** (read in full): pins `@mantine/charts`,
  `@mantine/core`, `@mantine/dates`, `@mantine/hooks` all at exact `9.5.2`.
  No `@mantine/dropzone` entry anywhere in `dependencies` or
  `devDependencies`.
- **`@mantine/dropzone@9.5.2` exists and is version-matched**, confirmed via
  `npm view @mantine/dropzone@9.5.2 peerDependencies` this session:
  ```json
  { "react": "^19.2.0", "react-dom": "^19.2.0",
    "@mantine/core": "9.5.2", "@mantine/hooks": "9.5.2" }
  ```
  Both peer bounds are exact matches for what's already pinned
  (`react`/`react-dom` `^19.2.0`, `@mantine/core`/`@mantine/hooks` `9.5.2`).
  `npm view @mantine/dropzone@9.5.2 dependencies` shows one transitive
  dependency, `react-dropzone@15.0.0`, pulled in automatically — not a
  second direct dependency this app would need to manage.
- **Mantine's `Dropzone` component** (fetched from `mantine.dev/x/dropzone/`
  this session): calls `onDrop(files: File[])` with accepted files and an
  `onReject` callback for files failing validation; `accept` takes MIME
  types or Mantine's exported grouped constants (`PDF_MIME_TYPE`,
  `MS_WORD_MIME_TYPE`, etc. — no built-in constant for `.pkpass`, which has
  no registered MIME type, so a custom accept value stays necessary either
  way); renders three status-conditional sub-components,
  `Dropzone.Accept`/`Dropzone.Reject`/`Dropzone.Idle`; is "based on
  react-dropzone and supports all of its core features." Confirmed
  separately: `activateOnClick` defaults to enabled — the dropzone's root
  area is itself clickable to open a native file browser, not
  drop-only — and can be explicitly disabled via `activateOnClick={false}`
  if a design wanted drop-only behavior (not proposed here).
- **`TicketEntryForm.test.tsx`** (read in full): every upload test locates
  the real hidden `<input type="file">` directly by its `accept` attribute
  (`getPkpassFileInput`/`getPdfFileInput`, `TicketEntryForm.test.tsx:42-50`)
  rather than by label, because Mantine's `FileInput` renders its visible,
  labelled element as an unlabelled `<button>`
  (`TicketEntryForm.test.tsx:28-41`'s own comment) — `getByLabelText`
  resolves to the button, not the input, and `fireEvent.change` on that
  input is what actually exercises the upload path. Every upload test then
  drives selection via `fireEvent.change(inputEl, { target: { files:
  [file] } })`.

## Decisions

### 1. Component choice: adopt `@mantine/dropzone`'s `Dropzone`, replacing `FileInput` in both upload panels

Three real alternatives:

- **Keep `FileInput`, add drag-and-drop as a wrapper around it** (e.g. a
  `<div>` with native `onDragOver`/`onDrop` handlers surrounding the
  existing `FileInput`, feeding a dropped file into the same `onChange`
  path). **Rejected.** `FileInput`'s underlying `<input type="file">` has no
  API to programmatically set a `FileList` from a drop event in a way the
  input itself will treat as a real user selection (browsers reject
  assigning to `input.files` directly outside very narrow paths) — this
  wrapper would end up needing its own separate `File`-handling code path
  next to `FileInput`'s, duplicating validation/accept-matching logic that
  `Dropzone`/`react-dropzone` already provide, for no benefit over just
  using the real component built for this.
- **Native HTML5 drag-and-drop, no new dependency at all** (raw
  `onDragOver`/`onDragLeave`/`onDrop` handlers on a plain `<div>`, reading
  `event.dataTransfer.files`, with a separate, hand-rolled click-to-browse
  `<input>` for the keyboard/click fallback). **Rejected.** This is a real,
  workable option in isolation, but it means hand-building exactly what
  `@mantine/dropzone` already ships and tests: drag-state styling hooks
  (`Dropzone.Accept`/`Reject`/`Idle`), accept-type matching, size
  validation, and the click-to-browse fallback (Decision 5) — for a
  library this app already has zero new-category exposure to adopting
  (`@mantine/charts`, `@mantine/dates`, `@mantine/hooks` are already exact-
  pinned dependencies of the same monorepo/release train). Rolling this by
  hand would be net-more code, for a strictly narrower feature set, in
  exchange for avoiding one already-well-fitting first-party package.
- **`@mantine/dropzone`'s `Dropzone` component, replacing `FileInput` in
  both `UploadPanel` instances.** **Chosen.** Version-exact peer match
  confirmed above (no version-skew risk, no `--force`/`--legacy-peer-deps`
  needed); same design system as every other input already on this form
  (`TextInput`, `Tabs`, `Alert`, `Button` are all `@mantine/core`), so the
  visual language stays consistent without new bespoke styling; ships both
  interaction modes (drag-drop and click-to-browse) in one component
  (Decision 5), which is exactly the two modes this form needs to keep
  supporting.

`FileInput` itself is not kept anywhere in `TicketEntryForm.tsx` afterward —
Decision 5 establishes `Dropzone`'s built-in click fallback covers the
click-only interaction `FileInput` used to provide, so there is no
"both controls, pick one" state to design around.

### 2. Drop-target scope: each `Dropzone` replaces its own tab's `FileInput` control — not the whole form, not the whole page

Three scopes were considered:

- **The whole page becomes a drop target once any part of it is visible**
  (e.g. dropping a file anywhere on `/track/mine` triggers upload, even if
  `TicketEntryForm` is still collapsed). **Rejected.** This means designing
  answers for "the form isn't open yet" (auto-open, per the brief's own
  framing) *and* "which of two upload tabs should this route to" (Decision
  3) *and* a global drop listener that has to coexist with every other
  interactive element already on `/track/mine` (`AttachTicketAction`,
  `TrackedTrainListRow` links, `TicketEntryForm` itself) without capturing
  drops meant for none of them. It also has no story for `TicketPanel`'s two
  render sites, which share a page with `TrainJourney` and
  `DeleteTrainButton` — a page-wide drop target there would need to ignore
  drops anywhere near those, for a benefit (not having to first click "Add a
  ticket") that doesn't clearly outweigh the ambiguity.
- **The whole (open) `TicketEntryForm` becomes a drop target, auto-switching
  to whichever upload tab matches the dropped file's type, regardless of
  which tab is currently selected.** Considered seriously — this is the
  closest to "just drop the file on the form, it figures out the rest."
  **Rejected**, in favor of the narrower option below, for one concrete
  reason: it requires the manual-entry tab's own panel (text inputs, no
  file concept) to also somehow behave as a drop target and route elsewhere,
  which is a bigger, vaguer surface than this feature needs — a manual-entry
  `TextInput` accepting a stray file drop is not an interaction any part of
  this brief asked for, and Mantine's `Dropzone` is not designed to overlay
  arbitrary sibling form controls that aren't part of it.
- **Each upload tab's own `UploadPanel` becomes exactly one `Dropzone`,
  replacing that tab's `FileInput` 1:1** — i.e. the `pkpass` tab's drop
  target only ever calls `handleUpload(file, 'pkpass')`, the `pdf` tab's
  only ever calls `handleUpload(file, 'pdf')`, same as today's fixed-`kind`
  `FileInput`s. **Chosen.** This is the minimal, precise scope change: two
  existing controls swap implementation, nothing about the tabbed
  structure, the manual tab, or the collapsed/`open` gating changes at all.
  A user already on the `pkpass` tab drags a `.pkpass` file onto that tab's
  visible drop area — exactly the same gesture shape `FileInput`'s
  click-to-browse already asked for, just via drag instead of click.

**The form does not auto-open on a page-level drop, and does not
auto-switch tabs based on a dropped file's type.** Both of those behaviors
belong to the "whole form/page is a drop target" scopes rejected above, not
to this one. A user must still click the entry-point button
(`TicketEntryForm.tsx:277`) to open the form and pick the tab matching their
file, exactly as today — dragging changes *how* a file reaches the already-
visible control, not *when* the form appears or *which* tab is active. This
keeps `open`/`tab` state fully unchanged and keeps the "wrong tab" question
(dropping a PDF onto the `.pkpass` tab) answered by existing, already-tested
`accept`-based rejection (Decision 3) rather than a new auto-routing
mechanism.

### 3. File-type detection and validation on drop: no auto-detection — `accept` continues to scope each `Dropzone` exactly as it scopes each `FileInput` today, `kind` stays a per-tab constant

Because Decision 2 keeps two separate, tab-scoped drop targets (not one
unified one), there is no dropped-file-could-be-either-kind case to design
routing for. Each `Dropzone` takes the same `accept` value its `FileInput`
predecessor already receives — `.pkpass` for one, `application/pdf`-shaped
(Mantine's `PDF_MIME_TYPE` constant, or the equivalent literal, is a
reasonable direct swap) for the other — and `Dropzone`'s own `accept`
matching (inherited from `react-dropzone`) rejects a mismatched file before
it ever reaches `onDrop`'s accepted-files callback, routing instead to
`onReject`. `handleUpload(file, kind)`'s `kind` parameter is unchanged: still
a literal `'pkpass'`/`'pdf'` fixed per `UploadPanel` instance, never derived
from the dropped file's actual extension or MIME type.

A user who drags a PDF onto the `.pkpass` tab sees Mantine's
`Dropzone.Reject` state (or, in the simplest first-pass rendering, no
special-cased in-DOM reject styling at all — that's a rendering-polish
choice, not a data-flow one) — no request reaches `handleUpload`/the
backend at all, mirroring how today's `FileInput` already can't be handed a
`.pdf` file on the `.pkpass` tab either (`accept=".pkpass"` on the browser's
native file picker already filters the same way, just via the OS dialog's
own filtering instead of a drag-reject). This means the backend's existing
`422` "could not read this as a train .pkpass" handling
(`TicketEntryForm.tsx:156-161`) stays the actual backstop for a
same-extension-but-wrong-content file (e.g. a `.pkpass`-named file that
isn't really one) exactly as it is today — `accept` filtering, on drop or on
click, was never a content-validation step, only an extension/MIME
pre-filter.

**Considered and rejected: a single, unified `Dropzone` per upload tab
group** (one drop target that accepts both `.pkpass` and PDF, sniffs the
dropped file's extension/MIME client-side, and calls `handleUpload` with the
inferred `kind`). Rejected for the same reason the "whole form is a drop
target" option was rejected in Decision 2: it's a materially bigger,
vaguer surface (what happens on an ambiguous or unrecognized extension?)
for a benefit — one drop target instead of two — that isn't what the
existing tabbed structure already asks the user to do (pick a tab, then
provide a file for that tab). Two-tabs-two-drop-targets keeps parity with
the form's existing information architecture rather than introducing a new
one alongside it.

### 4. Error handling: unchanged, by design

`handleUpload`'s status-code branching (`TicketEntryForm.tsx:144-176`) —
`200`/`401`/`400`/`422`/`504`/`413`/generic-fallback — operates entirely on
the `Response` from the same `fetch(`/api/${ticketsBasePath}/${kind}`, ...)`
call it makes today. Nothing about *how* the `File` object reached
`handleUpload` (a `FileInput`'s `onChange`, or a `Dropzone`'s `onDrop`) is
visible to or used by that function — its signature, `handleUpload(file:
File | null, kind: 'pkpass' | 'pdf')`, stays exactly as it is. This spec
proposes **zero changes** to `handleUpload`, to `UploadPanel`'s `error`
prop/rendering (`TicketEntryForm.tsx:408-420`), or to any of the six
outcome branches. The only new failure mode drag-and-drop introduces —
`Dropzone`'s `onReject` firing for a file that doesn't match `accept` — is a
client-side, pre-`handleUpload` rejection (Decision 3), not a change to the
server-round-trip error handling this section already covers.

### 5. Accessibility/keyboard fallback: `Dropzone` alone covers both interaction modes — `FileInput` is not kept as a separate fallback

Confirmed this session (`mantine.dev/x/dropzone/`, fetched directly, not
assumed from prior familiarity): `Dropzone`'s `activateOnClick` prop
defaults to enabled, meaning the rendered drop area is itself a click
target that opens the native file-picker dialog — the same end action
`FileInput`'s click already performs today. Mantine's own docs describe an
`openRef`-based "open file browser manually" pattern for cases wanting an
explicit external trigger button, which isn't needed here since
`activateOnClick`'s default already gives click-to-browse without extra
wiring.

Mantine's `Dropzone` page itself has no dedicated accessibility/keyboard
section (confirmed by a direct fetch of that page this session — no such
heading exists), and a direct check of `react-dropzone`'s own docs (which
`Dropzone` wraps and, per its own docs, "supports all of its core
features") did not surface an explicit keyboard-activation guarantee in the
excerpts fetched this session, though `react-dropzone` does document a
`noKeyboard` option to *disable* keyboard interaction — its existence as an
opt-out implies keyboard activation (focus + Enter/Space triggering the
file dialog) is present by default when not set, but this spec does not
assert that as independently confirmed fact. **This is flagged as needing a
concrete check during implementation**: before removing `FileInput`
entirely, confirm in the running app (or via a fresh, targeted doc/source
check) that `Dropzone`'s default rendering is keyboard-focusable and
activatable without a mouse — if it is not, `noKeyboard={false}` (or
whatever the equivalent explicit prop turns out to be) needs to be set
explicitly rather than assumed as an already-correct default. Either way,
**no separate `FileInput` fallback is proposed** — the design intent is one
control per tab, not two coexisting ones, so this check is a verification
step on `Dropzone`'s own default behavior, not a decision between keeping
or dropping `FileInput`.

## Architecture

Before:

```
UploadPanel (kind, accept)
  └─ FileInput  accept={accept}  onChange={(file) => onFile(file, kind)}
       (click opens native picker; no drag-and-drop)
```

After (same shape, one control swapped per tab — no change to `TicketEntryForm`'s
`Tabs`/`open`/`tab` state, or to anything outside `UploadPanel`):

```
UploadPanel (kind, accept)
  └─ Dropzone  accept={accept}  onDrop={([file]) => onFile(file, kind)}
       onReject={...}  (mismatched file type -- Decision 3, no request sent)
       (drag-and-drop AND click-to-browse, per Decision 5)
```

Call-site tree (confirmed this session, corrected from the brief per
"Corrections" above) — unchanged by this spec, shown to make explicit where
the two swapped `Dropzone`s actually surface:

```
/track/mine  (app/track/mine/page.tsx:113)
  └─ TicketEntryForm (no trackingId -- standalone ticket)
       └─ UploadPanel × 2  (pkpass, pdf)  -- Dropzone here

/train/by-id/[trackingId]  (page.tsx:57)
/train/[uid]/[date]        (page.tsx:58)
  └─ TicketPanel
       └─ TicketEntryForm (trackingId set)   -- 0-ticket or N-ticket variant
            └─ UploadPanel × 2  (pkpass, pdf)  -- Dropzone here

/track  (app/track/page.tsx)
  └─ TrackTrainForm only -- TicketEntryForm is NOT rendered here (Corrections)
```

## Testing

`TicketEntryForm.test.tsx` currently drives every upload path via
`fireEvent.change(inputEl, { target: { files: [file] } })` against the real
hidden `<input type="file">`, located directly by its `accept` attribute
rather than by label (`TicketEntryForm.test.tsx:28-50`), because
`FileInput`'s visible labelled element is an unlabelled `<button>`. Swapping
to `Dropzone` changes what needs to be simulated:

- **What likely still works unchanged**: `react-dropzone` (which `Dropzone`
  wraps) also renders a real hidden `<input type="file" accept="...">` for
  its click-to-browse path (Decision 5) — if that input keeps the same
  `accept`-attribute shape, the existing `getPkpassFileInput`/
  `getPdfFileInput` query helpers and every existing `fireEvent.change(...)`
  call in the current test file plausibly continue to exercise the
  click-to-browse path with no change at all. This needs confirming against
  `Dropzone`'s actual rendered DOM once implementation starts, not assumed
  here.
- **What is new and needs its own coverage**: a *drop*-sourced selection is
  a different DOM event shape than a `change` event on an `<input>` —
  simulating it means firing a synthetic `drop` (and/or `dragEnter`)
  `DataTransfer`-bearing event at the `Dropzone`'s root element, not at the
  hidden input. `react-dropzone`'s own test suite is the natural reference
  for the exact event/`DataTransfer` shape it expects (e.g. constructing an
  event with a `dataTransfer.files`/`dataTransfer.items` payload), but this
  spec does not pin down the precise Testing-Library incantation — **flagged
  as needing investigation during implementation**, per this task's own
  allowance, since it depends on exactly how `react-dropzone@15.0.0`
  reads the drop event internally (`items` vs `files`, `DataTransfer` vs a
  plain object), which is worth confirming against the installed version's
  actual behavior under jsdom rather than guessed here.
- **New test cases this addition needs, regardless of the exact drop-event
  mechanics above**:
  - A drop-sourced file on the `pkpass` tab reaches `handleUpload` with
    `kind: 'pkpass'` and produces the same observable outcomes the existing
    click-sourced tests already assert (200 → tab switches to manual,
    fields pre-filled per the existing `it.each` status-code table).
  - Same, for the `pdf` tab.
  - A file that doesn't match a tab's `accept` (e.g. a `.pdf` dropped on the
    `.pkpass` tab) does **not** call `fetch` at all — asserting
    `onReject`'s effect rather than any `handleUpload` outcome, since
    Decision 3 keeps this a client-side pre-filter.
  - Every existing `it.each` status-code case (`TicketEntryForm.test.tsx:100-119`)
    stays green unmodified for the click-to-browse path — this addition
    should not need to touch or duplicate that table, only add drop-sourced
    variants alongside it.
- No new coverage is needed for `handleSubmit`, the manual tab, or any of
  the standalone-ticket (`describe('with no trackingId ...')`) tests —
  none of those paths touch file selection at all, per Decision 4.

## Explicitly out of scope

- **Auto-opening the collapsed form on a page-level drop.** Decision 2
  explicitly rejects a page-wide or whole-form drop target; the entry-point
  button remains the only way to open the form.
- **Auto-switching tabs based on a dropped file's detected type.** Decision
  2/3 explicitly reject unified/auto-routing drop targets; each tab's drop
  area only ever produces its own fixed `kind`.
- **Any change to `handleUpload`'s status-code handling, `UploadPanel`'s
  error rendering, or the manual-entry tab.** Decision 4; this spec touches
  only the file-selection control in front of already-unmodified logic.
- **`/track` (`TrackTrainForm`).** Corrections, above: `TicketEntryForm`
  isn't rendered there today, so there is nothing to add drag-and-drop to
  on that page as part of this feature.
- **Multi-file drop/upload.** `Dropzone` supports multi-file selection by
  default; this form's one-ticket-per-submission shape (one `handleUpload`
  call, one `FormData` with a single `file` field,
  `TicketEntryForm.tsx:133-134`) is unchanged — implementation should
  constrain each `Dropzone` to a single accepted file (e.g. `maxFiles={1}`
  or taking only `files[0]` from `onDrop`), not designed further here since
  it's a direct, mechanical carry-over of the existing one-file assumption.
- **Visual design of the drop area itself** (exact copy, icon, sizing,
  `Dropzone.Accept`/`Reject`/`Idle` styling). Left to implementation; this
  spec fixes the data-flow and scope decisions, not the pixel-level look.

## Open questions / risks

1. **`Dropzone`'s default keyboard-accessibility behavior is not
   independently confirmed** (Decision 5) — this needs a concrete check
   (running app or source-level) before `FileInput` is removed, since this
   spec's whole "no separate fallback needed" stance depends on it being
   true.
2. **The exact drop-event simulation shape for the new tests is unresolved
   here** (Testing, above) — needs a short investigation against the
   installed `react-dropzone@15.0.0`'s real jsdom behavior once
   implementation starts.
3. **Whether `Dropzone`'s hidden click-to-browse `<input>` preserves the
   same `accept`-attribute-based query pattern** the existing test suite
   relies on (`getPkpassFileInput`/`getPdfFileInput`) is assumed likely but
   not verified against actual rendered DOM in this session — worth a quick
   check early in implementation so the existing test helpers either keep
   working unmodified or get updated deliberately, not by surprise.
4. **`react-dropzone@15.0.0` arrives as a new transitive dependency**
   (`@mantine/dropzone@9.5.2`'s only listed dependency, confirmed via `npm
   view` this session). Not a direct dependency this app manages, and not a
   new *category* of dependency (this app already ships several
   `@mantine/*` companion packages), but it is still new code in the
   bundle/lockfile — noted here for completeness, not treated as a blocker.
