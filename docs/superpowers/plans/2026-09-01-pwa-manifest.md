# PWA Manifest and Icons Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make this app installable — a native `app/manifest.ts`, two new static icon PNGs, a light/dark `viewport.themeColor` pair, and exactly one Apple-specific meta tag (`apple-mobile-web-app-status-bar-style: 'black-translucent'`) — with no service worker and no change to the app's runtime data-fetching or caching behaviour.

**Architecture:** A new `frontend/app/manifest.ts` (Next's file-convention route, auto-linked into `<head>` the same way `icon.svg`/`apple-icon.png` already are) returns a static `MetadataRoute.Manifest` object literal. Two new static PNGs (`frontend/public/icon-192.png`, `frontend/public/icon-512.png` — this repo's first `public/` directory) are rasterized once from the existing `frontend/app/icon.svg` artwork via a one-off `sharp` script, mirroring exactly how `frontend/app/apple-icon.png` was produced. `frontend/app/layout.tsx` gains a `themeColor` field on its `viewport` export and an `appleWebApp` field on its existing `metadata` export — both additive edits to objects that already exist or are being introduced by a sibling, independently-planned feature (see Global Constraints).

**Tech Stack:** Next.js 16 App Router `manifest.ts`/`Metadata`/`Viewport` file-convention APIs (no new dependency); `sharp` (already present, transitively, as Next's own optional dependency — `frontend/package-lock.json:4127,4773`, `"sharp": "^0.34.5"`), used once as a standalone script, not added to `package.json`; Vitest, following `frontend/app/globals.test.ts`'s precedent of asserting directly against a file's exported/parsed content rather than rendering anything.

**Spec:** `docs/superpowers/specs/2026-09-01-pwa-manifest-design.md` — read in full before starting; this plan carries its Decisions into concrete tasks and does not restate its research. Cross-references below to "Decision N" refer to that document.

**Status note — every citation below re-confirmed directly against this worktree's current source, not trusted blind from the spec:**

- `frontend/app/layout.tsx` (157 lines, confirmed by `wc -l`): `import type { Metadata } from 'next';` at line 5; `export const metadata: Metadata = { title: 'Distant Signal', description: '...' }` at lines 16-20 (description text confirmed to match the spec's citation verbatim, including the em dash and "first-class handling of operators whose routes share trunk track" clause); no `viewport` export exists anywhere in the file today. `<head>` (lines 92-95) contains only `<ColorSchemeScript defaultColorScheme="auto" />`; `<MantineProvider theme={theme} defaultColorScheme="auto">` opens at line 97, `<AutoRefresh />` at line 98.
- `frontend/app/icon.svg` (39 lines, confirmed by direct read): 32×32 `viewBox`, no explicit `width`/`height` attributes. Gradient stops at lines 22-23: `#be4bdb` (Mantine grape-6) → `#9c36b5` (grape-8). Background `<rect>` at line 27: `x="1" y="1" width="30" height="30" rx="8"` — rounded corners, 1px transparent margin inset from the 32×32 viewBox, not full-bleed. Signal post `<rect>` at line 30. Fishtail-arm `<polygon>` (Mantine yellow-4, `#ffd43b`) at lines 34-38.
- `frontend/app/apple-icon.png`: confirmed exactly **180×180**, **2994 bytes**, by direct PNG IHDR-chunk read this session (`struct.unpack('>II', ...)`, same method the spec used). Its provenance, from `git show 35cc693` (commit `35cc6935fa54acba6744596b9b3d51788b79236d`, "Add a favicon: a fishtail-notched distant-signal arm in grape"): *"Also add app/apple-icon.png (180x180, opaque, generated from an equivalent full-bleed SVG via sharp, which is already present as a transitive dependency)."* Confirmed this session: **no script was ever committed** — `git log --all -p` across every commit and every `*.js`/`*.mjs`/`*.ts` file in this repo's history turns up no `sharp(...)` call and no generator script anywhere; the exact original command is genuinely not recoverable, only its description. Task 1 below specifies a precise equivalent, not a guess at the lost original.
- `frontend/app/globals.test.ts` (222 lines, read in full): this repo's established precedent for testing a non-component, data-shaped file directly — plain assertions against parsed/imported content, no rendering (e.g. `readFileSync('app/globals.css', 'utf8')` + regex assertions). `frontend/app/manifest.test.ts`/the `layout.test.tsx` extension below follow the same "no rendering, assert directly on the returned/exported value" spirit, adapted for TypeScript module exports rather than a CSS file read.
- `frontend/app/layout.test.tsx` (35 lines, confirmed by `wc -l`, read in full): imports only `{ TrackedTrainsNavItem } from './layout'` (line 4) today; no test of `metadata` or any other export exists yet.
- **No `frontend/public/` directory exists** — confirmed by directory listing. `frontend/next.config.mjs` (confirmed by read) has no `images`/asset config relevant to this change.
- **No local `node_modules` in this worktree** — confirmed (`ls frontend/node_modules` fails). `sharp` is only reachable after `npm install` pulls in `next`'s own optional dependency tree; Task 1 accounts for this.
- Next.js `MetadataRoute.Manifest`/`Viewport.themeColor`/`Metadata.appleWebApp` field shapes (snake_case manifest fields, `{ media, color }[]` theme-color form, independently-rendering `appleWebApp` sub-fields) are per the spec's own citations against `node_modules/next@16.2.10`'s type definitions (`dist/lib/metadata/types/manifest-types.d.ts`, `metadata-interface.d.ts:157,619`, `metadata.js:627-660`) — this worktree has no local `node_modules` to independently re-verify those against this session, so they are carried forward from the spec's own citations, not re-confirmed here.

## Global Constraints

- **No new dependency, anywhere.** `sharp` is already present transitively via `next`'s own optional dependency (`package-lock.json:4127,4773`). Do not add it to `frontend/package.json`. No new npm package for anything else in this plan either.
- **No service worker, offline caching, or push notification code, anywhere in this plan.** Per the spec's own Recommendation and "Explicitly out of scope" — unchanged, not re-litigated.
- **Exact copy/values, not paraphrases** — copy these verbatim, character for character:
  - `name`: `'Distant Signal'`
  - `short_name`: `'Distant Signal'` (same string, no abbreviation — Decision 1's "no invented shorthand" call is final for this plan; the 14-vs-~12-character platform guidance gap is an accepted, documented limitation, not something to solve here)
  - `description`: `'A personal UK rail companion: live line status, train tracking, and ticket/Delay-Repay support.'`
  - `start_url`: `'/'`
  - `display`: `'standalone'`
  - `background_color`: `'#ffffff'`
  - `theme_color`: `'#be4bdb'`
  - `icons`: exactly `[{ src: '/icon-192.png', sizes: '192x192', type: 'image/png' }, { src: '/icon-512.png', sizes: '512x512', type: 'image/png' }]` — no `purpose` field, no third maskable icon, no `apple-icon.png` entry (Decision 1/2).
  - `viewport.themeColor`: exactly `[{ media: '(prefers-color-scheme: light)', color: '#ffffff' }, { media: '(prefers-color-scheme: dark)', color: '#242424' }]` (Decision 3).
  - `metadata.appleWebApp`: exactly `{ statusBarStyle: 'black-translucent' }` — **never** add `capable`/`mobile-web-app-capable` or `title`/`apple-mobile-web-app-title`; both are explicitly rejected by the spec's Decision 4 (the former now actively discouraged, the latter pure duplication of `short_name`). If any task's implementer is tempted to add either "while they're at it," that is out of scope — do not.
- **`icon-192.png`/`icon-512.png` are direct re-exports of `icon.svg`'s existing artwork, not a redesign, and not a full-bleed variant.** Unlike `apple-icon.png` (which needed a separate full-bleed SVG because iOS composites alpha onto black), Chrome's default `purpose: "any"` icons have no such requirement — rasterize `icon.svg` exactly as authored, rounded corners and transparent margin included (Decision 2).
- **`public/` is new to this repo — first directory of its kind.** Both new icons go under `frontend/public/`, not `frontend/app/` (a hyphenated filename like `icon-192.png` isn't one of Next's recognized special `app/`-level filenames and wouldn't be served at all from there).
- **`frontend/app/icon.svg` and `frontend/app/apple-icon.png` are unchanged by this entire plan.** No task edits either file's content.
- **This plan's `viewport` export edit may need to compose with a sibling, independently-planned feature touching the same object — do not silently overwrite it.** `docs/superpowers/plans/2026-09-01-dynamic-color-scheme-meta.md` (also unimplemented as of this plan's writing — confirmed by this session's own read of `layout.tsx`, which has no `viewport` export today) adds `export const viewport: Viewport = { colorScheme: 'light' };` to this exact same file, at this exact same location (immediately after the `metadata` export). The two plans add **different fields** to the same object (`colorScheme` vs. `themeColor`) and are not in conflict conceptually — the sibling spec explicitly scoped `theme-color` **out** of its own work, naming this spec's territory as a deliberate, planned follow-on, not a collision. But whichever plan lands second must **add its field to the `viewport` object the first plan already created**, not declare a second `export const viewport`, which would be a duplicate-export compile error. Task 3 below gives explicit steps for both landing orders. If, when Task 3 is executed, neither plan has landed yet, either order is fine; if the color-scheme plan's Task 1 has already landed, Task 3's "existing `viewport` export" branch applies.
- **Testing convention:** colocated `*.test.ts`/`*.test.tsx`, Vitest, run via `npm test` (from `frontend/`). Every task's verification step runs `npm test` and `npm run build` (both from `frontend/`) and requires both to pass with no new failures — `npm run build` matters here specifically because it's the only step that exercises Next's real manifest/metadata pipeline (a malformed `MetadataRoute.Manifest` return type, a bad icon path, or a `Viewport`/`Metadata` shape mismatch are all `tsc`/`next build`-time concerns, not something `npm test` alone would catch).
- **Parallelizable tasks:** Task 1 (icon generation) and Task 2 (`manifest.ts`) touch disjoint files and can be dispatched in parallel — `manifest.ts`'s `icons` array references `/icon-192.png`/`/icon-512.png` by path only, so it type-checks whether or not the files exist yet, though both should land before Task 4's manual verification. Task 3 (`layout.tsx`) is independent of Tasks 1-2 and can run in parallel with either, subject to the `viewport`-composition constraint above if the sibling plan is landing concurrently. Task 4 (manual verification) depends on Tasks 1-3 all being landed.

## Not in this plan

- **`display: 'standalone'` removes the browser back button, and several inner pages have no in-app back link.** The spec's own Open question 1 flags this as a real, currently-unaddressed navigation gap this feature newly exposes: `frontend/app/lines/[id]/page.tsx` and `frontend/app/stations/[crs]/page.tsx` (both directly reachable from the home page's pinned lists) have no "back to home"/"back to list" link in their own markup, per the spec's own check. No task in this plan adds one. Flagged here so it isn't silently lost — worth a follow-up plan, not solved by this one.
- **A maskable icon variant, `ImageResponse`/`icon.tsx` dynamic generation, an iOS install-prompt UI nudge, and extending `ColorSchemeMeta` to post-mount-sync the theme-color tags for a manual `ThemeToggle` override** — all explicitly out of scope per the spec's own "Explicitly out of scope" section; unchanged here, not re-litigated.

---

### Task 1: Icon generation — `public/icon-192.png` and `public/icon-512.png`

**Files:**
- Create: `frontend/public/icon-192.png`
- Create: `frontend/public/icon-512.png`

**Interfaces:**
- Produces: two static PNG files at root-relative paths `/icon-192.png` and `/icon-512.png`, referenced by Task 2's `manifest.ts`.
- **Depends on:** nothing — foundational, and inert on its own until Task 2's `manifest.ts` references these paths.

The original `apple-icon.png` script was never committed (Status note above) — this task specifies a precise equivalent for direct rasterization of `icon.svg`'s existing artwork (no full-bleed variant needed, unlike `apple-icon.png` — see Global Constraints), not a reconstruction of a lost exact command.

- [ ] **Step 1: Install dependencies (this worktree has no local `node_modules`)**

Run: `cd frontend && npm install`
Expected: completes with `sharp` present at `frontend/node_modules/sharp` (it's `next`'s own optional transitive dependency, not something this step adds to `package.json`).

- [ ] **Step 2: Create the `public/` directory**

Run: `mkdir -p frontend/public`
Expected: this is the repo's first `public/` directory (Global Constraints).

- [ ] **Step 3: Write the one-off generation script**

Write `frontend/generate-pwa-icons.mjs` (temporary — deleted in Step 6, never committed, matching Decision 2's "one-off script, not a build step"):

```js
import sharp from 'sharp';
import { readFileSync } from 'node:fs';

const svg = readFileSync('app/icon.svg');

// icon.svg's viewBox is 32x32 with no explicit width/height, so its
// intrinsic rasterization size at sharp/librsvg's default density is
// ambiguous. Rather than compute the exact density needed to hit 192px/
// 512px directly, render at a fixed density well above both targets
// (1000) so `.resize()` below always downsamples from a much
// higher-resolution raster -- never upscales a blurry one. This sidesteps
// needing to know librsvg's exact default-intrinsic-size-from-viewBox
// behaviour precisely, at the one-time cost of a slightly slower render.
const DENSITY = 1000;

for (const size of [192, 512]) {
  await sharp(svg, { density: DENSITY })
    .resize(size, size)
    .png()
    .toFile(`public/icon-${size}.png`);
  console.log(`wrote public/icon-${size}.png`);
}
```

- [ ] **Step 4: Run it**

Run: `cd frontend && node generate-pwa-icons.mjs`
Expected: prints `wrote public/icon-192.png` and `wrote public/icon-512.png`; both files now exist under `frontend/public/`.

- [ ] **Step 5: Verify dimensions directly (same method used to confirm `apple-icon.png`'s size this session)**

Run:
```bash
python3 -c "
import struct
for size, path in [(192, 'frontend/public/icon-192.png'), (512, 'frontend/public/icon-512.png')]:
    with open(path, 'rb') as f:
        data = f.read()
    w, h = struct.unpack('>II', data[16:24])
    assert (w, h) == (size, size), f'{path}: expected {size}x{size}, got {w}x{h}'
    print(path, w, h, 'OK')
"
```
Expected: both files print `OK` at their respective sizes. If either fails, the density in Step 3 was insufficient or `.resize()`'s output shape is wrong — do not proceed until both pass.

- [ ] **Step 6: Delete the one-off script**

Run: `rm frontend/generate-pwa-icons.mjs`
Expected: the script is gone; only the two PNGs remain as new files. Confirm with `git status` (from repo root) that `frontend/generate-pwa-icons.mjs` does not appear and `frontend/public/icon-192.png`/`frontend/public/icon-512.png` do.

- [ ] **Step 7: Commit**

```bash
git add frontend/public/icon-192.png frontend/public/icon-512.png
git commit -m "Add 192x192 and 512x512 PWA manifest icons, rasterized from icon.svg via sharp"
```

---

### Task 2: `app/manifest.ts`

**Files:**
- Create: `frontend/app/manifest.ts`
- Test: `frontend/app/manifest.test.ts`

**Interfaces:**
- Produces: `frontend/app/manifest.ts`'s default export, `manifest(): MetadataRoute.Manifest`, returning the static object literal below. Next auto-discovers and links this the same way it already does `icon.svg`/`apple-icon.png` — no manual `<link>` and no import anywhere else in the app.
- **Depends on:** nothing at compile time (references `/icon-192.png`/`/icon-512.png` by string path only) — recommended to land after or alongside Task 1 so the referenced files actually exist by the time this is manually verified (Task 4).

- [ ] **Step 1: Write the failing test**

Create `frontend/app/manifest.test.ts`:

```typescript
import { describe, it, expect } from 'vitest';
import manifest from './manifest';

describe('manifest', () => {
  it('names the app "Distant Signal" for both name and short_name, unabbreviated', () => {
    const result = manifest();
    expect(result.name).toBe('Distant Signal');
    expect(result.short_name).toBe('Distant Signal');
  });

  it('uses the trimmed description, distinct from layout.tsx metadata.description', () => {
    expect(manifest().description).toBe(
      'A personal UK rail companion: live line status, train tracking, and ticket/Delay-Repay support.',
    );
  });

  it('starts at the site root', () => {
    expect(manifest().start_url).toBe('/');
  });

  it('renders standalone with the app\'s light background and grape-6 brand theme colour', () => {
    const result = manifest();
    expect(result.display).toBe('standalone');
    expect(result.background_color).toBe('#ffffff');
    expect(result.theme_color).toBe('#be4bdb');
  });

  it('declares exactly the two required icons, 192x192 and 512x512, image/png, no purpose field', () => {
    expect(manifest().icons).toEqual([
      { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
      { src: '/icon-512.png', sizes: '512x512', type: 'image/png' },
    ]);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run (from `frontend/`): `npm test -- manifest.test.ts`
Expected: FAIL — `./manifest` module doesn't exist yet.

- [ ] **Step 3: Write `manifest.ts`**

Create `frontend/app/manifest.ts`:

```typescript
import type { MetadataRoute } from 'next';

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: 'Distant Signal',
    short_name: 'Distant Signal',
    description:
      'A personal UK rail companion: live line status, train tracking, and ticket/Delay-Repay support.',
    start_url: '/',
    display: 'standalone',
    background_color: '#ffffff',
    theme_color: '#be4bdb',
    icons: [
      { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
      { src: '/icon-512.png', sizes: '512x512', type: 'image/png' },
    ],
  };
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run (from `frontend/`): `npm test -- manifest.test.ts`
Expected: PASS, all five assertions.

- [ ] **Step 5: Run the production build**

Run (from `frontend/`): `npm run build`
Expected: PASS — this is the step that actually exercises Next's `MetadataRoute.Manifest` type-check and auto-discovery of `app/manifest.ts`; `npm test` alone doesn't touch Next's own build pipeline.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/manifest.ts frontend/app/manifest.test.ts
git commit -m "Add app/manifest.ts: PWA manifest with Distant Signal branding and the two new icons"
```

---

### Task 3: `layout.tsx` — `viewport.themeColor` and `metadata.appleWebApp`

**Files:**
- Modify: `frontend/app/layout.tsx`
- Modify: `frontend/app/layout.test.tsx`

**Interfaces:**
- Produces: `viewport.themeColor` (light/dark `{ media, color }[]` pair) and `metadata.appleWebApp` (`{ statusBarStyle: 'black-translucent' }`) on `frontend/app/layout.tsx`'s existing/new `Metadata`/`Viewport` exports.
- **Depends on:** nothing at compile time. **Composition note:** check the current state of `frontend/app/layout.tsx` before starting this task's Step 3 — see the two branches below, which depend on whether `docs/superpowers/plans/2026-09-01-dynamic-color-scheme-meta.md`'s Task 1 has already landed in this worktree (Global Constraints).

- [ ] **Step 1: Write the failing tests**

In `frontend/app/layout.test.tsx`, change the import on line 4 from:

```typescript
import { TrackedTrainsNavItem } from './layout';
```

to:

```typescript
import { TrackedTrainsNavItem, viewport, metadata } from './layout';
```

Add two new top-level `describe` blocks (this file already imports `describe`/`it`/`expect` from `vitest` at line 1):

```typescript
describe('viewport.themeColor', () => {
  it('pairs the light-scheme white background with the dark-scheme #242424 body colour', () => {
    // Only asserts the themeColor field specifically -- not a full-object
    // equality check on `viewport` -- so this test doesn't break if a
    // sibling feature (docs/superpowers/plans/2026-09-01-dynamic-color-scheme-meta.md)
    // has also added a `colorScheme` field to the same object.
    expect(viewport.themeColor).toEqual([
      { media: '(prefers-color-scheme: light)', color: '#ffffff' },
      { media: '(prefers-color-scheme: dark)', color: '#242424' },
    ]);
  });
});

describe('metadata.appleWebApp', () => {
  it('sets only statusBarStyle to black-translucent -- no capable, no title', () => {
    // Exact-shape check, not just a `.statusBarStyle` field check: this is
    // the one place this plan's Global Constraints must hold structurally
    // -- `capable`/`title` must never be added alongside this.
    expect(metadata.appleWebApp).toEqual({ statusBarStyle: 'black-translucent' });
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- layout.test.tsx`
Expected: FAIL — `viewport` is not exported from `./layout` (unless the sibling color-scheme plan already added it, in which case this specific failure is `viewport.themeColor` being `undefined`), and `metadata.appleWebApp` is `undefined`.

- [ ] **Step 3: Add `appleWebApp` to the existing `metadata` export**

In `frontend/app/layout.tsx`, extend the existing `metadata` export (currently lines 16-20):

```typescript
export const metadata: Metadata = {
  title: 'Distant Signal',
  description:
    'A personal UK rail companion: TfL-style line status, live train tracking, and ticket/Delay-Repay support — with first-class handling of operators whose routes share trunk track, so an incident is only ever flagged on the lines it actually affects.',
  appleWebApp: {
    statusBarStyle: 'black-translucent',
  },
};
```

Only the new `appleWebApp` field is added; `title`/`description` are unchanged, verbatim.

- [ ] **Step 4: Add or extend the `viewport` export — pick the branch that matches this worktree's current `layout.tsx`**

**Branch A — no `viewport` export exists yet** (the state confirmed by this plan's own Status note; the common case if this plan lands before the sibling color-scheme plan): add the import and a new export.

Change line 5 from:

```typescript
import type { Metadata } from 'next';
```

to:

```typescript
import type { Metadata, Viewport } from 'next';
```

Then add immediately after the `metadata` export from Step 3:

```typescript
export const viewport: Viewport = {
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: '#ffffff' },
    { media: '(prefers-color-scheme: dark)', color: '#242424' },
  ],
};
```

**Branch B — a `viewport` export already exists** (the sibling color-scheme plan's Task 1 landed first, adding `export const viewport: Viewport = { colorScheme: 'light' };` right after `metadata`): do **not** declare a second `export const viewport` — that is a duplicate-export compile error. Instead add the `themeColor` field to the existing object:

```typescript
export const viewport: Viewport = {
  colorScheme: 'light',
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: '#ffffff' },
    { media: '(prefers-color-scheme: dark)', color: '#242424' },
  ],
};
```

(`Viewport` will already be imported in this branch — the sibling plan's own Task 1 adds it — so no import change is needed here.)

- [ ] **Step 5: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- layout.test.tsx`
Expected: PASS, including the pre-existing `TrackedTrainsNavItem` tests (unaffected by this change).

- [ ] **Step 6: Run the production build**

Run (from `frontend/`): `npm run build`
Expected: PASS — confirms no duplicate-export error from Step 4's branch choice and that `Metadata.appleWebApp`/`Viewport.themeColor` type-check against Next's real types.

- [ ] **Step 7: Commit**

```bash
git add frontend/app/layout.tsx frontend/app/layout.test.tsx
git commit -m "Add viewport.themeColor (light/dark) and metadata.appleWebApp.statusBarStyle to layout.tsx"
```

---

### Task 4: Manual verification (not unit-testable)

**Files:** none — this task runs the built app and inspects it directly; it makes no code changes.

**Interfaces:**
- Consumes: the fully wired feature from Tasks 1-3.
- **Depends on:** Tasks 1, 2, and 3 all landed.

Per the spec's own Testing section (Decision 5): whether "Add to Home Screen" actually produces a correctly-iconed, correctly-titled, chrome-less launch on a real iOS device, and whether Chrome DevTools'/Lighthouse's installability audit passes, are not unit-testable in this codebase's existing Vitest/jsdom setup — no task in this plan invents automated coverage for either. This task names them as manual steps plainly, rather than skipping them.

- [ ] **Step 1: Build and run the app locally**

Run (from `frontend/`): `npm run build && npm run start`

- [ ] **Step 2: Run Chrome DevTools' / Lighthouse's installability audit**

Open the running app in Chrome, run a Lighthouse audit (or DevTools' Application panel → Manifest), and confirm: the manifest is detected, `name`/`short_name`/`icons`/`start_url`/`display` all show the values from Task 2, and both icon files load without a 404.

- [ ] **Step 3: Verify "Add to Home Screen" on real iOS hardware or Simulator**

Add the app to the home screen on an actual iOS device or Simulator. Confirm: the home-screen icon matches `apple-icon.png` (unchanged by this plan — Global Constraints), the installed app's title reads "Distant Signal," and launching it opens in standalone (chrome-less) mode.

- [ ] **Step 4: Confirm the status bar tint**

With the app installed and open in standalone mode, confirm the status bar takes its tint from the page's own background (white in light mode, `#242424` in dark mode) rather than showing the default black/white bar — this is `apple-mobile-web-app-status-bar-style: 'black-translucent'` (Task 3) actually working.

- [ ] **Step 5: Record the outcome**

No commit is produced by this task. If any check surfaces an unexpected result (wrong icon, manifest not detected, status bar not tinting), do not silently ship — raise it against the spec's own Decisions above, since Tasks 1-3's own automated tests already independently confirmed the underlying values are correct; a failure here points at a platform/build-pipeline issue, not a wrong static value.
