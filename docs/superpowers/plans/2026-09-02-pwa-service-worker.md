# PWA Service Worker and Offline Caching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a hand-rolled `frontend/public/sw.js` service worker implementing a strict default-deny, allowlist-only cache-first strategy for Next's content-hashed static output, the two manifest icons, `/manifest.webmanifest`, and one static `/offline.html` fallback — with every other request (critically, every navigation and RSC-refresh request that carries this app's live status data) passed straight to the network with no caching and no stale fallback — plus the build-time cache-busting and registration/lifecycle plumbing that makes it deployable and updatable, and fix a real, already-confirmed production bug (`frontend/Dockerfile`'s `runtime-prod` stage silently dropping `public/`) as a hard prerequisite.

**Architecture:**

```
frontend/Dockerfile ─────────────────────────────────────────── Task 1
  MODIFIED: restores COPY .../public ./public (independent of
  everything else; fixes a real, already-shipped 404 for the
  manifest icons, which this plan's own new public/ files would
  otherwise inherit)

frontend/public/sw-cache-rules.js ─────────┐                     Task 2
  NEW: isCacheable(pathname) -- the one     │
  pure, Vitest-testable piece of logic      │
  frontend/public/sw-cache-rules.test.js    │
  NEW                                       │
                                             ├──▶ frontend/public/sw.js ─┐  Task 4
frontend/public/offline.html ───────────────┘    NEW: install/activate/ │
  NEW: static, zero-framework fallback page        fetch, importScripts │  Task 3
  (Task 3, independent of Task 2)                  ('/sw-cache-rules.js')│
                                                                          │
                              ┌───────────────────────────────────────────┘
                              ▼
                    frontend/next.config.mjs ── MODIFIED: headers() for   Task 5
                    frontend/scripts/            Cache-Control: no-cache
                      stamp-sw-version.mjs ── NEW  on /sw.js; build script
                    frontend/package.json ── MODIFIED  wires the stamp
                                                        script in after
                                                        `next build`

frontend/components/ServiceWorkerRegister.tsx ── NEW               Task 6
frontend/components/ServiceWorkerRegister.test.tsx ── NEW          (parallel
frontend/app/layout.tsx ── MODIFIED: mounts it                     with 4/5)

frontend/e2e/service-worker.spec.ts ── NEW: first real use of the  Task 7
  already-configured, currently-empty frontend/e2e/ Playwright     (needs
  suite -- registration/activation, precache contents, a real       1, 4, 5, 6)
  offline navigation, a still-fails-offline mutation request

Task 8: final verification (vitest + tsc + next build + docker build
  sanity re-check + npm run test:e2e), all tasks landed
```

**Tech Stack:** Plain, dependency-free JavaScript for `public/sw.js`/`public/sw-cache-rules.js`/`public/offline.html` (no Workbox/next-pwa/Serwist, no build step for these files themselves) + a small dependency-free Node build script (`node:fs`/`node:path`/`node:url` only) + Next.js App Router (`headers()`, a Client Component registered in `RootLayout`) + `@mantine/hooks`' `useMounted` (already the established mount-gating hook — `PrideToggle.tsx`, `LastUpdated.tsx`, `ColorSchemeMeta.tsx`) + Vitest/`@testing-library/react` for the two genuinely unit-testable pieces + Playwright (`frontend/e2e/`, configured but unused until this plan) for everything that needs a real `ServiceWorkerGlobalScope`/Cache Storage/simulated offline network.

**Spec:** `docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md` — read in full before starting; this plan does not restate its research, only carries its Decisions into concrete tasks. Cross-references below to "Decision N" refer to that document. The sibling spec `docs/superpowers/specs/2026-09-02-line-status-notifications-design.md` is referenced for coordination only (Global Constraints) — this plan does not implement anything from it.

## Global Constraints

- **Default-deny, allowlist-only caching — never a blocklist, never elsewhere weakened.** Exactly five URL shapes are cache-first-cacheable: `/_next/static/*` (prefix), `/icon-192.png`, `/icon-512.png`, `/manifest.webmanifest`, `/offline.html`. Every other request — every navigation, every RSC-refresh fetch, every `/api/*` call, anything not on this list — is network-only, with the single exception of the offline-navigation fallback below. No task may widen this list without updating `sw-cache-rules.test.js`'s explicit false-case coverage in the same change (Decision 1).
- **No service-worker-generating library, ever** — no `next-pwa`, no `@ducanh2912/next-pwa`, no Serwist, no Workbox, no new runtime `dependencies`/`devDependencies` entry of any kind for this feature. The one new file that isn't hand-written application logic (`scripts/stamp-sw-version.mjs`) uses only Node's own `fs`/`path`/`url` modules (Decision 2).
- **Never cache or offline-serve any live content, under any strategy, staleness-labelled or not.** The only offline fallback this feature provides is the single static `/offline.html` page, served only when a *navigation* request (`request.mode === 'navigate'`) fails at the network layer — never a reconstruction of any previously-viewed real page (Decision 1, Decision 3).
- **`frontend/Dockerfile`'s missing `COPY .../public ./public` line in `runtime-prod` is a hard, blocking prerequisite (Task 1), not optional cleanup.** Confirmed still true by direct read of the current Dockerfile this session (see Task 1) — the two existing manifest icons are almost certainly already 404ing in the deployed image today, and this plan's own new `public/sw.js`/`sw-cache-rules.js`/`offline.html` would silently inherit the same fate if Task 1 isn't done first.
- **`install`/`activate`/`fetch` in `sw.js` stay three independent, flat `self.addEventListener(...)` blocks in one plain file — no shared helper abstraction or closure state spanning them beyond the two top-level constants (`CACHE_NAME`, `PRECACHE_URLS`) and the imported `isCacheable`.** This is a direct, explicit coordination point with a sibling effort: **`docs/superpowers/specs/2026-09-02-line-status-notifications-design.md` is being planned/implemented concurrently, by a separate track, and depends on this exact `sw.js` file existing.** It specifies a `{ title, body, url, tag }` push payload contract and expects to add `self.addEventListener('push', ...)` and `self.addEventListener('notificationclick', ...)` to this same file later, as two more independent blocks touching none of the three this plan adds. No task in this plan implements `push`/`notificationclick` handling — that is explicitly out of scope here — but no task may refactor `install`/`activate`/`fetch` into a shape (e.g., a single dispatch function, a shared mutable module-level flag those handlers would need to know about) that makes adding those two listeners later anything other than "paste two more `addEventListener` blocks at the bottom of the file."
- **`sw.js` and `sw-cache-rules.js` are classic (non-module) scripts, loaded via `importScripts()`, not `import`/`export`.** Verified via web search this session: Chrome, Edge, and Safari support `type: 'module'` service workers, but **Firefox does not**, as of this writing — an actively open Mozilla bug (`bugzilla.mozilla.org` #1360870), not a shipped feature. Since this app's own sibling push-notification spec explicitly targets desktop Firefox as a supported push platform, `sw.js` cannot be registered as an ES module. This has one concrete consequence for `sw-cache-rules.js` (Task 2): it cannot use top-level `export`/`import` syntax (a `SyntaxError` in a classic script evaluated via `importScripts()`), so it exposes `isCacheable` via a plain `self.isCacheable = isCacheable` / `module.exports = { isCacheable }` dual assignment instead — see Task 2 for the exact code and why both branches are needed (one for `importScripts()` in the real service worker, one for Vitest's CJS-interop import in the test).
- **Cache-name generation (`CACHE_NAME`) is stamped from `.next/BUILD_ID` at build time (Decision 5) — never hand-maintained, never left as a fixed string in committed source.** `frontend/public/sw.js` ships with a literal `__BUILD_ID__` placeholder; `frontend/scripts/stamp-sw-version.mjs` substitutes the real value as an added step in `package.json`'s `build` script, run after `next build` (so `.next/BUILD_ID` exists). **Note for whoever runs this locally:** because the script rewrites the checked-in `frontend/public/sw.js` in place, running `npm run build` outside Docker (e.g. to sanity-check the stamping step) will leave a real-BUILD_ID diff in your working tree — `git checkout -- frontend/public/sw.js` to discard it, don't commit a stamped value.
- **`activate` purges every cache name except the current `CACHE_NAME`, and both `self.skipWaiting()` (in `install`) and `self.clients.claim()` (in `activate`) are used** — a new deploy's service worker takes control immediately rather than waiting for every open tab to close (Decision 5).
- **`next.config.mjs`'s `headers()` sets `Cache-Control: no-cache` (not `no-store`) on `/sw.js` specifically, and no other path** — this keeps the browser's own HTTP cache from masking a real `sw.js` byte-content change across deploys, without forcing a full re-download on every request (Decision 5).
- **Registration is `navigator.serviceWorker.register('/sw.js')` with no explicit `scope` option** — root-level registration path, maximum origin scope, deliberate (Decision 4; also needed by the sibling push spec's future `notificationclick` deep-linking).
- **Testing convention:** colocated `*.test.tsx`/`*.test.ts`/`*.test.js`, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`) for every Client Component test, Vitest (`npx vitest run` from `frontend/`) for pure-logic and component tests, Playwright (`npx playwright test` / `npm run test:e2e` from `frontend/`) for anything needing a real browser (`ServiceWorkerGlobalScope`, Cache Storage, `context.setOffline()`). Every task's verification step runs the relevant file(s) directly. Task 8 runs the full suite plus `npx tsc --noEmit` plus `npm run build` plus `npm run test:e2e`.
- **Parallelizable tasks:** Task 1 (Dockerfile) depends on nothing in this plan and touches no file any other task touches — dispatchable at any point. Tasks 2 and 3 depend on nothing and touch disjoint files — parallelizable with each other and with Task 1. Task 4 depends on Tasks 2 and 3. Task 5 depends on Task 4 (needs `sw.js`'s `CACHE_NAME` placeholder to exist). Task 6 touches only `components/ServiceWorkerRegister.tsx`/`.test.tsx` and `app/layout.tsx` — no file overlap with Tasks 4/5, so it is dispatchable in parallel with them once Tasks 2/3 have landed (it does not literally import anything from `sw.js`, only registers the string path `/sw.js`). Task 7 depends on Tasks 1, 4, 5, and 6 all having landed (it exercises the full registered/precached/offline flow end to end). Task 8 depends on every other task.

---

### Task 1: Fix `frontend/Dockerfile`'s missing `public/` COPY in `runtime-prod`

**Files:**
- Modify: `frontend/Dockerfile`

**Interfaces:**
- Produces: a `runtime-prod` image whose `/app/public` directory is present and populated, matching what `builder-prod` actually built. Consumed by every later task in this plan whose new files live under `frontend/public/` (Tasks 2, 3, 4) and by the two already-shipped manifest icons.
- **Depends on:** nothing — foundational, independent, blocking.

Confirmed by direct read of the current file this session: `frontend/Dockerfile:74-81` copies `.next`, `node_modules`, `package.json` from `builder-prod` into `runtime-prod`, then a comment block says *"No `public/` directory: this plan adds no static assets... If a later change adds static assets under `frontend/public/`, add `COPY --from=builder-prod --chown=frontend:frontend /app/public ./public` back in at that point."* That comment is stale: `frontend/public/icon-192.png` and `icon-512.png` were added by commit `86b26b6` (the PWA manifest spec's implementation) and this COPY line was never restored. `frontend/public/` is not excluded by `.dockerignore` (confirmed — only `frontend/node_modules` and `frontend/.next` are listed there), and no volume mount or other mechanism serves `public/` in `runtime-prod` at deploy time (confirmed by reading `docker-compose.yml` — the default, no-override compose file used in production has no `frontend` volume entry at all; only `docker-compose.dev.yml`, which is not used in production, bind-mounts the whole tree). So the two existing icons are almost certainly already 404ing in the deployed image today, undetected.

- [ ] **Step 1: Restore the COPY line and update the stale comment**

Replace (`frontend/Dockerfile:76-81`):

```dockerfile
COPY --from=builder-prod --chown=frontend:frontend /app/package.json ./package.json
# No `public/` directory: this plan adds no static assets (no favicon,
# no images). Next.js's `public/` dir is optional — omitting this COPY
# is correct, not a shortcut. If a later change adds static assets under
# `frontend/public/`, add `COPY --from=builder-prod --chown=frontend:frontend
# /app/public ./public` back in at that point.
```

with:

```dockerfile
COPY --from=builder-prod --chown=frontend:frontend /app/package.json ./package.json
# public/ now holds real, load-bearing static assets: the PWA manifest
# icons (icon-192.png/icon-512.png, commit 86b26b6) and, as of
# docs/superpowers/plans/2026-09-02-pwa-service-worker.md, sw.js/
# sw-cache-rules.js/offline.html. The comment this replaced ("no public/
# directory... add COPY back if one appears later") went stale when the
# manifest icons landed without this line being restored -- meaning both
# icons have likely been silently 404ing in the deployed image ever
# since (invisible in `next dev`, which reads public/ straight off disk
# regardless of this Dockerfile). See
# docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md's
# "Current relevant state," last bullet.
COPY --from=builder-prod --chown=frontend:frontend /app/public ./public
```

- [ ] **Step 2: Build the `runtime-prod` target and verify `public/` is actually present in the image**

Run from the repo root:

```bash
docker build -f frontend/Dockerfile --target runtime-prod -t distant-signal-frontend-test .
docker run --rm --entrypoint ls distant-signal-frontend-test -la public/
```

Expected: the second command lists `icon-192.png` and `icon-512.png` (this task lands before Tasks 2-4 add `sw.js`/`sw-cache-rules.js`/`offline.html`, so those aren't expected here yet — Task 8 re-runs this same check once every file exists). Before Step 1's fix, the same `docker run` command would fail with `ls: cannot access 'public/': No such file or directory` — confirming the bug was real; running the build against the pre-fix Dockerfile once (e.g. via `git stash` then `git stash pop`) is a good sanity check if you want to see the failure directly, but is not required to proceed.

- [ ] **Step 3: Commit**

```bash
git add frontend/Dockerfile
git commit -m "Fix frontend/Dockerfile: restore the public/ COPY step runtime-prod silently dropped"
```

---

### Task 2: `frontend/public/sw-cache-rules.js` — the allowlist matcher, unit-tested

**Files:**
- Create: `frontend/public/sw-cache-rules.js`
- Create: `frontend/public/sw-cache-rules.test.js`

**Interfaces:**
- Produces: `isCacheable(pathname: string): boolean`, exposed two ways from the same file — `self.isCacheable` (for `sw.js`'s `importScripts('/sw-cache-rules.js')`, Task 4) and `module.exports = { isCacheable }` (for this task's own Vitest test, and any future test that needs it).
- **Depends on:** nothing — foundational, independent.

This is Decision 1's entire caching decision, extracted into one small, pure, directly-testable function — the one piece of this feature the design spec's own Testing section calls out as earning real unit coverage, since it's what most directly guards the "never accidentally cache live content" property.

- [ ] **Step 1: Write the failing test, `frontend/public/sw-cache-rules.test.js`**

```javascript
import { describe, it, expect } from 'vitest';
import cacheRules from './sw-cache-rules.js';

const { isCacheable } = cacheRules;

describe('isCacheable', () => {
  it.each([
    ['/_next/static/chunks/main-abc123.js', true],
    ['/_next/static/css/app-def456.css', true],
    ['/icon-192.png', true],
    ['/icon-512.png', true],
    ['/manifest.webmanifest', true],
    ['/offline.html', true],
  ])('%s is cacheable', (pathname, expected) => {
    expect(isCacheable(pathname)).toBe(expected);
  });

  it.each([
    ['/', false],
    ['/lines/123', false],
    ['/api/preferences', false],
    ['/api/Train/track', false],
    // Close-but-not-actually-matching shapes, guarding against an
    // overly loose prefix/substring check rather than an exact
    // pathname comparison:
    ['/icon-192.png/evil', false],
    ['/notmanifest.webmanifest', false],
    ['/sw.js', false],
  ])('%s is NOT cacheable', (pathname, expected) => {
    expect(isCacheable(pathname)).toBe(expected);
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd frontend && npx vitest run public/sw-cache-rules.test.js`
Expected: FAIL — `Failed to resolve import "./sw-cache-rules.js"` (the file doesn't exist yet).

- [ ] **Step 3: Write `frontend/public/sw-cache-rules.js`**

```javascript
// Plain, dependency-free JS -- deliberately no `import`/`export` syntax.
// This file is loaded two different ways that disagree on module syntax:
// sw.js (Task 4) loads it via the classic importScripts(), which throws a
// SyntaxError on any top-level `export`/`import` keyword (this app's
// service worker must stay a classic, non-module script -- Firefox does
// not support `type: 'module'` service workers as of this writing, unlike
// Chrome/Edge/Safari; see this plan's Global Constraints). This test file
// loads it as a CommonJS-shaped module via Vitest/Vite's own CJS-interop.
// The two conditional assignments below satisfy both call sites from one
// shared file, with no build step for either.
//
// See docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md
// Decision 1 -- default-deny, allowlist-only. Forgetting to add a new
// static asset here is a cache miss (safe, caught immediately in
// testing); it can never mean live content gets cached by omission,
// because nothing here is a blocklist.

/**
 * @param {string} pathname - a URL's pathname, e.g. new URL(request.url).pathname
 * @returns {boolean} true only for the five cache-first-cacheable URL
 *   shapes; false for everything else, including every navigation/
 *   RSC-refresh request and every /api/* call.
 */
function isCacheable(pathname) {
  if (pathname.startsWith('/_next/static/')) return true;
  return (
    pathname === '/icon-192.png' ||
    pathname === '/icon-512.png' ||
    pathname === '/manifest.webmanifest' ||
    pathname === '/offline.html'
  );
}

if (typeof self !== 'undefined') {
  self.isCacheable = isCacheable;
}
if (typeof module !== 'undefined' && module.exports) {
  module.exports = { isCacheable };
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd frontend && npx vitest run public/sw-cache-rules.test.js`
Expected: PASS (13 cases).

- [ ] **Step 5: Commit**

```bash
git add frontend/public/sw-cache-rules.js frontend/public/sw-cache-rules.test.js
git commit -m "Add sw-cache-rules.js: the service worker's allowlist matcher, unit-tested"
```

---

### Task 3: `frontend/public/offline.html` — static, zero-framework fallback page

**Files:**
- Create: `frontend/public/offline.html`

**Interfaces:**
- Produces: a static file at `/offline.html`, served cache-first once precached (Task 4's `install` handler) and served by `sw.js`'s `fetch` handler on a failed navigation request.
- **Depends on:** nothing — independent, static content only.

Per Decision 3: fully inlined CSS, no dependency on `_next/static/*` chunks, no Mantine, no React runtime — this is the one page in the app that must render even if every other precache entry is somehow missing. Reuses `app/error.tsx`'s tone (heading, one line of dimmed explanatory text, one retry button) translated to plain HTML/CSS, and reuses the `LastUpdated.tsx`/`DataFreshnessInfo` *concept* (a captured timestamp + a live "time since" recomputation) via a small inline vanilla-JS snippet reading `localStorage['lastSuccessfulLoadAt']` (written by Task 6's `ServiceWorkerRegister`). Uses the app's brand colour (`#ae3ec9`, the WCAG-AA-passing grape-7 `lib/theme.ts`/`app/globals.css` already establish as the accessible anchor colour) rather than inventing a new palette.

- [ ] **Step 1: Write `frontend/public/offline.html`**

```html
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Distant Signal — Offline</title>
<style>
  :root { color-scheme: light dark; }
  body {
    margin: 0;
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
    background: #ffffff;
    color: #1a1a1a;
  }
  @media (prefers-color-scheme: dark) {
    body { background: #242424; color: #f1f1f1; }
  }
  main { max-width: 28rem; padding: 2rem; text-align: center; }
  h1 { font-size: 1.5rem; margin: 0 0 0.5rem; }
  p { color: #6b6b6b; margin: 0 0 1.5rem; }
  @media (prefers-color-scheme: dark) { p { color: #a8a8a8; } }
  button {
    font: inherit;
    padding: 0.6rem 1.4rem;
    border: none;
    border-radius: 4px;
    background: #ae3ec9;
    color: #ffffff;
    cursor: pointer;
  }
  button:hover { background: #9c35b5; }
</style>
</head>
<body>
<main>
  <h1>You&rsquo;re offline</h1>
  <p id="offline-message">Distant Signal needs a connection to show current line status.</p>
  <button type="button" onclick="location.reload()">Try again</button>
</main>
<script>
  // Reuses LastUpdated.tsx's mechanic (a stored absolute timestamp, "now"
  // recomputed at render/tick time) without reusing the component itself --
  // this page has no React runtime. If no value is stored yet (first-ever
  // visit, offline before any successful load), the line is simply
  // omitted -- no fabricated fallback timestamp. Written by
  // ServiceWorkerRegister.tsx on every successful navigation/RSC-refresh
  // (see that component's doc comment).
  (function () {
    try {
      var last = window.localStorage.getItem('lastSuccessfulLoadAt');
      if (!last) return;
      var lastDate = new Date(last);
      if (isNaN(lastDate.getTime())) return;

      var messageEl = document.getElementById('offline-message');
      var baseText = 'Distant Signal needs a connection to show current line status.';

      function render() {
        var diffMs = Date.now() - lastDate.getTime();
        var mins = Math.floor(diffMs / 60000);
        var relative;
        if (mins < 1) relative = 'less than a minute ago';
        else if (mins === 1) relative = '1 minute ago';
        else if (mins < 60) relative = mins + ' minutes ago';
        else {
          var hours = Math.floor(mins / 60);
          relative = hours === 1 ? '1 hour ago' : hours + ' hours ago';
        }
        messageEl.textContent = baseText + ' Last connected around ' + relative + '.';
      }

      render();
      setInterval(render, 30000);
    } catch (e) {
      // localStorage can throw (private browsing / storage blocked) --
      // the page still renders correctly with the fixed base message set
      // in markup above.
    }
  })();
</script>
</body>
</html>
```

- [ ] **Step 2: Open it directly to sanity-check it renders standalone**

Run: `cd frontend && python3 -m http.server 8123 --directory public` (or any static file server), then open `http://localhost:8123/offline.html` in a browser.
Expected: "You're offline" heading, explanatory text with no "Last connected" sentence (no `localStorage` value yet on a fresh browser profile), a working "Try again" button. Manually run `localStorage.setItem('lastSuccessfulLoadAt', new Date(Date.now() - 5 * 60000).toISOString())` in the browser console and reload — expected: message now ends "Last connected around 5 minutes ago."

- [ ] **Step 3: Commit**

```bash
git add frontend/public/offline.html
git commit -m "Add offline.html: static, zero-framework fallback page for a failed navigation"
```

---

### Task 4: `frontend/public/sw.js` — the hand-rolled service worker

**Files:**
- Create: `frontend/public/sw.js`

**Interfaces:**
- Consumes: `isCacheable` from `sw-cache-rules.js` (Task 2, via `importScripts`), `/offline.html` (Task 3).
- Produces: the registered service worker at `/sw.js`, consumed by `ServiceWorkerRegister.tsx` (Task 6, `register('/sw.js')`) and exercised end-to-end by Task 7's Playwright suite. Contains the `CACHE_NAME` placeholder Task 5's build script stamps.
- **Depends on:** Task 2, Task 3.

Implements Decision 1 (allowlist cache-first / default-deny network-only) and Decision 5 (build-scoped cache name, purge-on-activate, `skipWaiting`/`clients.claim`). Per this plan's Global Constraints: three flat, independent `addEventListener` blocks, structured so a later `push`/`notificationclick` handler (the sibling notifications spec's job, not this plan's) is purely additive.

- [ ] **Step 1: Write `frontend/public/sw.js`**

```javascript
// Hand-rolled, no build step, no Workbox/next-pwa/Serwist -- see
// docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md
// Decision 2. Classic (non-module) service worker script, loaded via
// importScripts() rather than `import` -- see this plan's Global
// Constraints on why (Firefox does not support `type: 'module'` service
// workers as of this writing).
//
// install/activate/fetch below are three independent addEventListener
// blocks, deliberately kept flat and simple: a later push/
// notificationclick handler (docs/superpowers/specs/
// 2026-09-02-line-status-notifications-design.md's job, a concurrent,
// separate effort -- not implemented here) is meant to be two more such
// blocks pasted at the bottom of this file, touching none of the three
// below.

importScripts('/sw-cache-rules.js');
const isCacheable = self.isCacheable;

// Stamped by scripts/stamp-sw-version.mjs at build time (Task 5),
// substituting .next/BUILD_ID's real value for this placeholder -- see
// Decision 5. Changing this string on every deploy is what makes this
// file's own bytes differ deploy-to-deploy, which both the browser's
// native SW-update check and the activate purge below depend on.
const CACHE_NAME = 'distant-signal-__BUILD_ID__';

// Precached eagerly on install. Deliberately NOT every /_next/static/*
// file -- there is no way for this hand-written file to know the current
// build's content-hashed filenames without a Workbox-style generated
// precache manifest, which Decision 2 explicitly rejects as unneeded
// machinery for this app's small static surface. /_next/static/* assets
// are instead cached lazily, the first time each is actually requested,
// by the cache-first branch in the fetch handler below -- still
// cache-first from the second request onward, just not pre-warmed here.
const PRECACHE_URLS = ['/icon-192.png', '/icon-512.png', '/manifest.webmanifest', '/offline.html'];

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(CACHE_NAME).then((cache) => cache.addAll(PRECACHE_URLS)));
  // Take over immediately rather than waiting for every open tab to
  // close -- Decision 5, point 3. Named trade-off: a tab that survives
  // past activate without a full reload could request an old-build-hashed
  // /_next/static/* chunk after the new SW has taken over; since this
  // fetch handler caches by exact/prefix URL match (not by "whatever the
  // current build's manifest says"), that request simply isn't in the new
  // precache and falls through to network -- a pre-existing Next.js
  // characteristic of any content-hashed-asset deploy, not new here.
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((names) => Promise.all(names.filter((name) => name !== CACHE_NAME).map((name) => caches.delete(name)))),
  );
  self.clients.claim();
});

self.addEventListener('fetch', (event) => {
  const { request } = event;
  // Only ever intercept GET -- every mutation (POST/PUT/DELETE, all
  // routed through /api/[...path]) is never allowlisted anyway (see
  // sw-cache-rules.js), but returning early here means this handler never
  // calls the Cache API on a request type it would reject.
  if (request.method !== 'GET') return;

  const url = new URL(request.url);

  if (isCacheable(url.pathname)) {
    event.respondWith(
      caches.match(request).then((cached) => {
        if (cached) return cached;
        return fetch(request).then((response) => {
          // Only cache a genuinely successful response -- all five
          // allowlisted shapes are same-origin, so a plain `response.ok`
          // check is sufficient (no opaque cross-origin response to
          // worry about here).
          if (response.ok) {
            const copy = response.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(request, copy));
          }
          return response;
        });
      }),
    );
    return;
  }

  // Default-deny (Decision 1): every non-allowlisted request -- every
  // navigation, every RSC-refresh fetch, every /api/* call, everything
  // else -- is passed straight to the network with no caching of the
  // response at all. The one exception is the navigation-failure fallback
  // immediately below, which serves the static offline SHELL, never a
  // reconstruction of previously-viewed real content.
  if (request.mode === 'navigate') {
    event.respondWith(fetch(request).catch(() => caches.match('/offline.html')));
    return;
  }

  // Every other non-allowlisted request (e.g. a PinToggle mutation, or a
  // failed AutoRefresh RSC-refresh fetch while genuinely offline): no
  // SW-level interception or fallback at all -- it fails exactly as it
  // already does today with no service worker present, by simply not
  // calling event.respondWith() here.
});
```

- [ ] **Step 2: Manual sanity check under `next dev`**

Run: `cd frontend && npm run dev`, open `http://localhost:3000` in a browser, open DevTools → Application → Service Workers.
Expected: `sw.js` registers (once Task 6 lands and mounts `ServiceWorkerRegister`; if running this check before Task 6, register it manually from the DevTools console instead: `navigator.serviceWorker.register('/sw.js')`), status becomes "activated and is running." DevTools → Application → Cache Storage should show one cache (`distant-signal-__BUILD_ID__`, since Task 5 hasn't stamped it yet in dev) containing exactly the four `PRECACHE_URLS` entries.

**Open question flagged by the design spec, worth a direct manual check here (not automated):** with DevTools' Network tab open, let the page sit for 30+ seconds so `AutoRefresh`'s `router.refresh()` fires, and confirm the resulting RSC-refresh request's path is not `/_next/static/*`, `/icon-192.png`, `/icon-512.png`, `/manifest.webmanifest`, or `/offline.html` — i.e. it correctly falls through to the network-only branch above, not the cache-first one. (The design's allowlist is robust to this by construction regardless of the exact request shape, but confirming it empirically once is cheap and closes Open question #1 from the design spec.)

- [ ] **Step 3: Commit**

```bash
git add frontend/public/sw.js
git commit -m "Add sw.js: hand-rolled service worker, default-deny allowlist caching"
```

---

### Task 5: Cache invalidation on deploy — `stamp-sw-version.mjs` + `next.config.mjs` `headers()`

**Files:**
- Create: `frontend/scripts/stamp-sw-version.mjs`
- Modify: `frontend/next.config.mjs`
- Modify: `frontend/package.json`

**Interfaces:**
- Consumes: `frontend/public/sw.js`'s `__BUILD_ID__` placeholder (Task 4), `.next/BUILD_ID` (written by `next build`).
- Produces: a `build` script that leaves `public/sw.js` stamped with the real build id after `npm run build`; an HTTP response header (`Cache-Control: no-cache`) on `/sw.js` specifically.
- **Depends on:** Task 4.

`frontend/scripts/` does not exist yet — this is a new directory, unlike the manifest spec's own icon-rasterization step, which was run ad hoc via `sharp` and never committed as a script file (confirmed by reading that commit, `86b26b6`: two binary PNGs added, zero script files). This is the first committed one-off Node script in `frontend/`, in spirit consistent with that ad hoc precedent (dependency-free, small, one job) but a genuinely new location.

- [ ] **Step 1: Write `frontend/scripts/stamp-sw-version.mjs`**

```javascript
#!/usr/bin/env node
// Dependency-free build-time script (see
// docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md
// Decision 5) -- Node's own fs/path/url modules only, no new
// devDependency. Run as an added step in package.json's `build` script,
// AFTER `next build` (so .next/BUILD_ID exists) and BEFORE the Docker
// image layer finalizes (so the stamped file, not the placeholder, is
// what ships) -- substitutes sw.js's CACHE_NAME placeholder with the real
// per-build id, so sw.js's own byte content changes on every deploy. This
// is what the browser's native SW-update check (a byte-for-byte
// comparison against the currently-installed worker) and sw.js's own
// `activate` purge (Task 4) both depend on to actually invalidate a prior
// deploy's cache.
//
// NOTE: this rewrites frontend/public/sw.js IN PLACE, a tracked source
// file. Running `npm run build` outside Docker (e.g. locally, to sanity-
// check this script) leaves a real-BUILD_ID diff in your working tree --
// `git checkout -- frontend/public/sw.js` to discard it before committing
// anything else.

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const buildIdPath = path.join(__dirname, '..', '.next', 'BUILD_ID');
const swPath = path.join(__dirname, '..', 'public', 'sw.js');

const PLACEHOLDER = '__BUILD_ID__';

const buildId = readFileSync(buildIdPath, 'utf8').trim();
const swSource = readFileSync(swPath, 'utf8');

if (!swSource.includes(PLACEHOLDER)) {
  throw new Error(
    `stamp-sw-version: ${swPath} does not contain the ${PLACEHOLDER} placeholder -- ` +
      "either it was already stamped by a previous run of this script against the same " +
      "checkout (see this file's own top comment), or sw.js's CACHE_NAME constant was " +
      'edited without preserving the placeholder.',
  );
}

writeFileSync(swPath, swSource.replace(PLACEHOLDER, buildId));
console.log(`stamp-sw-version: stamped public/sw.js with BUILD_ID ${buildId}`);
```

- [ ] **Step 2: Wire it into `package.json`'s `build` script**

Replace (`frontend/package.json`'s `scripts` block):

```json
    "build": "next build",
```

with:

```json
    "build": "next build && node scripts/stamp-sw-version.mjs",
```

- [ ] **Step 3: Add the `Cache-Control: no-cache` header for `/sw.js` in `frontend/next.config.mjs`**

Replace (`frontend/next.config.mjs:15-38`, the `nextConfig` declaration plus its export):

```javascript
/** @type {import('next').NextConfig} */
const nextConfig = {
  ...(devOrigins.length ? { allowedDevOrigins: devOrigins } : {}),
  // /track/tickets and /track/mine were two separate pages
  // (docs/superpowers/specs/2026-08-31-tickets-list-design.md,
  // docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md) until
  // Part B of the upload-first ticket-tracking plan merged them: once a
  // ticket can exist standalone (Part A), a bare "My Tickets" list sits
  // awkwardly next to a bare "My Tracked Trains" list, so `/track/mine` now
  // renders both. A config-level redirect (not a rendered stub page) keeps
  // any bookmarked/linked `/track/tickets` URL working rather than 404ing,
  // without maintaining a second copy of the merged page's content.
  async redirects() {
    return [
      {
        source: '/track/tickets',
        destination: '/track/mine',
        permanent: true,
      },
    ];
  },
};

export default nextConfig;
```

with:

```javascript
/** @type {import('next').NextConfig} */
const nextConfig = {
  ...(devOrigins.length ? { allowedDevOrigins: devOrigins } : {}),
  // /track/tickets and /track/mine were two separate pages
  // (docs/superpowers/specs/2026-08-31-tickets-list-design.md,
  // docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md) until
  // Part B of the upload-first ticket-tracking plan merged them: once a
  // ticket can exist standalone (Part A), a bare "My Tickets" list sits
  // awkwardly next to a bare "My Tracked Trains" list, so `/track/mine` now
  // renders both. A config-level redirect (not a rendered stub page) keeps
  // any bookmarked/linked `/track/tickets` URL working rather than 404ing,
  // without maintaining a second copy of the merged page's content.
  async redirects() {
    return [
      {
        source: '/track/tickets',
        destination: '/track/mine',
        permanent: true,
      },
    ];
  },
  // /sw.js's own byte content changes on every deploy (scripts/
  // stamp-sw-version.mjs stamps a fresh BUILD_ID into it) -- an
  // aggressively browser-HTTP-cached response could mask that from the
  // browser's own service-worker update check, which re-fetches this URL
  // on every navigation and does a byte-for-byte comparison. `no-cache`
  // (not `no-store`) still permits a cheap conditional revalidation
  // request rather than forcing a full re-download every time, while
  // guaranteeing the browser never trusts a locally-cached copy without
  // checking. See
  // docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md
  // Decision 5, point 4. This app is served directly via `next start`
  // with no CDN/ingress layer in front that would override response
  // headers (confirmed by reading charts/distant-signal/templates/ for
  // any cache-control/proxy-cache rule -- none exists), so this header
  // reaches the browser unmodified.
  async headers() {
    return [
      {
        source: '/sw.js',
        headers: [{ key: 'Cache-Control', value: 'no-cache' }],
      },
    ];
  },
};

export default nextConfig;
```

- [ ] **Step 4: Verify the build script stamps correctly**

Run:

```bash
cd frontend
npm run build
cat public/sw.js | grep CACHE_NAME
```

Expected: the `build` command's final step prints `stamp-sw-version: stamped public/sw.js with BUILD_ID <some id>`, and the `grep` shows `const CACHE_NAME = 'distant-signal-<that same id>';` — not the literal `__BUILD_ID__` placeholder. Then discard the stamped diff before continuing (see the script's own top comment): `git checkout -- frontend/public/sw.js`.

- [ ] **Step 5: Verify the `no-cache` header is actually sent**

Run:

```bash
cd frontend
npm run dev &
sleep 3
curl -sI http://localhost:3000/sw.js | grep -i cache-control
kill %1
```

Expected: `cache-control: no-cache` in the output.

- [ ] **Step 6: Commit**

```bash
git add frontend/scripts/stamp-sw-version.mjs frontend/next.config.mjs frontend/package.json
git commit -m "Stamp sw.js's cache name from .next/BUILD_ID at build time; no-cache header on /sw.js"
```

---

### Task 6: `ServiceWorkerRegister.tsx` — registration, mounted at root scope

**Files:**
- Create: `frontend/components/ServiceWorkerRegister.tsx`
- Create: `frontend/components/ServiceWorkerRegister.test.tsx`
- Modify: `frontend/app/layout.tsx`

**Interfaces:**
- Produces: `ServiceWorkerRegister({ loadedAt: string })` — a Client Component, mounted once in `RootLayout` alongside `AutoRefresh`/`ColorSchemeMeta`.
- **Depends on:** nothing at the file level (registers the string path `/sw.js`, doesn't import from it) — dispatchable in parallel with Tasks 4/5 once Tasks 2/3 have landed, per this plan's Global Constraints.

Per Decision 4: the same "side-effect-only, renders null, mounted once at root, `useMounted()`-gated" shape `AutoRefresh.tsx`/`ColorSchemeMeta.tsx` already establish — no new mounting convention. `register('/sw.js')` with no explicit `scope` (root scope, deliberate — see Global Constraints). A registration failure is caught and swallowed, same degrade-quietly shape `AuthNavItem`/`DataFreshnessNavItem` (`app/layout.tsx`) already use for a failed fetch in a root layout with no route-level `error.tsx`.

This component also owns writing `localStorage['lastSuccessfulLoadAt']` (consumed by `offline.html`, Task 3) — the concrete mechanism for Decision 3/4's "keeps it updated on each successful navigation/RSC-refresh tick": `RootLayout` (`app/layout.tsx`) is a **Server Component**, which re-executes on every navigation and every `AutoRefresh`-triggered `router.refresh()`. Passing a fresh `new Date().toISOString()` down as a prop on every one of those re-executions, and writing it to `localStorage` in a `useEffect` keyed on that prop, gives exactly the right signal: if the request behind a navigation/refresh had failed (genuinely offline), `RootLayout` never re-executes successfully and this component never receives a new `loadedAt` value, so the stored timestamp only ever advances on an actual successful server response — the same "no fabricated fallback" posture `LastUpdated.tsx` already takes for its own timestamp.

- [ ] **Step 1: Write the failing test, `frontend/components/ServiceWorkerRegister.test.tsx`**

```tsx
import { describe, it, expect, vi, afterEach } from 'vitest';
import { waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { renderWithMantine } from '@/test/render';
import { theme } from '@/lib/theme';
import { ServiceWorkerRegister } from './ServiceWorkerRegister';

const originalServiceWorker = Object.getOwnPropertyDescriptor(navigator, 'serviceWorker');

describe('ServiceWorkerRegister', () => {
  afterEach(() => {
    if (originalServiceWorker) {
      Object.defineProperty(navigator, 'serviceWorker', originalServiceWorker);
    } else {
      delete (navigator as unknown as Record<string, unknown>).serviceWorker;
    }
    window.localStorage.clear();
  });

  it('renders nothing', () => {
    delete (navigator as unknown as Record<string, unknown>).serviceWorker;
    const { container } = renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />);
    expect(container.querySelectorAll('*:not(style)')).toHaveLength(0);
  });

  it('registers /sw.js with no explicit scope when serviceWorker is supported', async () => {
    const register = vi.fn().mockResolvedValue({});
    Object.defineProperty(navigator, 'serviceWorker', { value: { register }, configurable: true });

    renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />);

    await waitFor(() => expect(register).toHaveBeenCalledWith('/sw.js'));
    expect(register).toHaveBeenCalledTimes(1);
  });

  it('does nothing (no throw) when serviceWorker is unsupported', () => {
    delete (navigator as unknown as Record<string, unknown>).serviceWorker;
    expect(() => renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />)).not.toThrow();
  });

  it('swallows a register() rejection without throwing', async () => {
    const register = vi.fn().mockRejectedValue(new Error('registration failed'));
    Object.defineProperty(navigator, 'serviceWorker', { value: { register }, configurable: true });

    expect(() => renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />)).not.toThrow();
    await waitFor(() => expect(register).toHaveBeenCalled());
  });

  it('writes loadedAt to localStorage["lastSuccessfulLoadAt"] on mount', async () => {
    delete (navigator as unknown as Record<string, unknown>).serviceWorker;
    renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />);
    await waitFor(() => expect(window.localStorage.getItem('lastSuccessfulLoadAt')).toBe('2026-09-02T10:00:00.000Z'));
  });

  it('updates localStorage again when loadedAt changes on a later render (a fresh successful navigation/refresh)', async () => {
    delete (navigator as unknown as Record<string, unknown>).serviceWorker;
    const { rerender } = renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />);
    await waitFor(() => expect(window.localStorage.getItem('lastSuccessfulLoadAt')).toBe('2026-09-02T10:00:00.000Z'));

    rerender(
      <MantineProvider theme={theme}>
        <ServiceWorkerRegister loadedAt="2026-09-02T10:00:30.000Z" />
      </MantineProvider>,
    );
    await waitFor(() => expect(window.localStorage.getItem('lastSuccessfulLoadAt')).toBe('2026-09-02T10:00:30.000Z'));
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd frontend && npx vitest run components/ServiceWorkerRegister.test.tsx`
Expected: FAIL — `Cannot find module './ServiceWorkerRegister'`.

- [ ] **Step 3: Write `frontend/components/ServiceWorkerRegister.tsx`**

```tsx
'use client';

import { useEffect } from 'react';
import { useMounted } from '@mantine/hooks';

/** Side-effect-only Client Component (renders nothing), mounted once in
 * RootLayout alongside AutoRefresh/ColorSchemeMeta -- the same
 * established "root-scope side effect, no new provider" shape those two
 * already use. See
 * docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md
 * Decision 4.
 *
 * register('/sw.js') with no explicit `scope` option is deliberate, not
 * an omission -- registering from a root-level path gives the service
 * worker the maximum possible scope (the whole origin), which both
 * sw.js's own allowlist (needs visibility into every navigation) and the
 * sibling line-status-notifications spec's future notificationclick
 * handler (needs a registration scope covering any in-app deep link)
 * require.
 *
 * A registration failure (unsupported browser, a sw.js fetch failure, a
 * syntax error in a bad deploy) is caught and swallowed -- same
 * degrade-quietly shape AuthNavItem/DataFreshnessNavItem (app/layout.tsx)
 * already use for a failed fetch in a root layout with no route-level
 * error.tsx. A broken registration must never break the page it's
 * mounted on; the app functions identically to today (no offline
 * support, no asset precaching) if this fails.
 *
 * `loadedAt`: a fresh ISO timestamp passed down from RootLayout -- a
 * Server Component, which re-executes on every navigation AND every
 * AutoRefresh-triggered router.refresh(). Recording
 * localStorage['lastSuccessfulLoadAt'] here, keyed on this prop actually
 * changing, is what gives offline.html's own "Last connected around X
 * ago" line a real, per-successful-load signal: if the request behind a
 * navigation/refresh had failed (genuinely offline), this component would
 * never receive a new `loadedAt` value in the first place, so the stored
 * timestamp only ever advances on an actual successful server response --
 * the same "no fabricated fallback" posture LastUpdated.tsx already takes
 * for its own timestamp. */
export function ServiceWorkerRegister({ loadedAt }: { loadedAt: string }) {
  const mounted = useMounted();

  useEffect(() => {
    if (!mounted) return;
    if ('serviceWorker' in navigator) {
      navigator.serviceWorker.register('/sw.js').catch(() => {
        // Swallowed -- see doc comment above.
      });
    }
  }, [mounted]);

  useEffect(() => {
    if (!mounted) return;
    try {
      window.localStorage.setItem('lastSuccessfulLoadAt', loadedAt);
    } catch {
      // localStorage can throw (private browsing / storage blocked) --
      // offline.html's own read of this key already tolerates a missing
      // value, so a failed write here is a silent no-op, not an error.
    }
  }, [mounted, loadedAt]);

  return null;
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd frontend && npx vitest run components/ServiceWorkerRegister.test.tsx`
Expected: PASS (6 tests).

- [ ] **Step 5: Mount it in `RootLayout`**

Replace (`frontend/app/layout.tsx:11-14`):

```tsx
import { AutoRefresh } from '@/components/AutoRefresh';
import { ColorSchemeMeta } from '@/components/ColorSchemeMeta';
import { OpenDataAttribution } from '@/components/OpenDataAttribution';
import { getDataFreshness, getSession } from '@/lib/api';
```

with:

```tsx
import { AutoRefresh } from '@/components/AutoRefresh';
import { ColorSchemeMeta } from '@/components/ColorSchemeMeta';
import { ServiceWorkerRegister } from '@/components/ServiceWorkerRegister';
import { OpenDataAttribution } from '@/components/OpenDataAttribution';
import { getDataFreshness, getSession } from '@/lib/api';
```

Replace (`frontend/app/layout.tsx:125-127`):

```tsx
        <MantineProvider theme={theme} defaultColorScheme="auto">
          <AutoRefresh />
          <ColorSchemeMeta />
```

with:

```tsx
        <MantineProvider theme={theme} defaultColorScheme="auto">
          <AutoRefresh />
          <ColorSchemeMeta />
          {/* RootLayout is a Server Component and re-executes on every
              navigation and every AutoRefresh-triggered router.refresh() --
              a fresh ISO timestamp here is what lets
              ServiceWorkerRegister record "last successful load" purely
              from receiving a new prop value; see that component's own
              doc comment. */}
          <ServiceWorkerRegister loadedAt={new Date().toISOString()} />
```

- [ ] **Step 6: Run the full layout test suite to confirm nothing broke**

Run: `cd frontend && npx vitest run app/layout.test.tsx`
Expected: PASS (unchanged — this task adds a new sibling component to the tree, touching none of `layout.test.tsx`'s existing assertions about `TrackedTrainsNavItem`/`viewport`/`metadata`).

- [ ] **Step 7: Commit**

```bash
git add frontend/components/ServiceWorkerRegister.tsx frontend/components/ServiceWorkerRegister.test.tsx frontend/app/layout.tsx
git commit -m "Add ServiceWorkerRegister, mounted in RootLayout alongside AutoRefresh/ColorSchemeMeta"
```

---

### Task 7: Playwright e2e coverage — the first real use of `frontend/e2e/`

**Files:**
- Create: `frontend/e2e/service-worker.spec.ts`

**Interfaces:**
- Consumes: the fully-assembled feature (Tasks 1, 4, 5, 6 all landed).
- **Depends on:** Tasks 1, 4, 5, 6.

Per the design spec's own Testing section: `install`/`activate`/`fetch` glue is not Vitest-testable (no natural `ServiceWorkerGlobalScope` host in `jsdom`) — this is the repo's first genuine use case for its already-configured but currently-empty `frontend/e2e/` Playwright suite (`frontend/playwright.config.ts` already points `testDir` at `./e2e` and has a `chromium` project + `webServer` wired to `next dev`).

- [ ] **Step 1: Write `frontend/e2e/service-worker.spec.ts`**

```typescript
import { test, expect } from '@playwright/test';

test.describe('service worker registration and precaching', () => {
  test('registers and activates on the home page', async ({ page }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));

    const registration = await page.evaluate(() => navigator.serviceWorker.getRegistration());
    expect(registration).toBeTruthy();
  });

  test('precaches exactly the four allowlisted static-asset URLs on install', async ({ page }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));

    const cachedPathnames = await page.evaluate(async () => {
      const names = await caches.keys();
      const pathnames: string[] = [];
      for (const name of names) {
        const cache = await caches.open(name);
        const requests = await cache.keys();
        pathnames.push(...requests.map((r) => new URL(r.url).pathname));
      }
      return pathnames;
    });

    expect(cachedPathnames.sort()).toEqual(
      ['/icon-192.png', '/icon-512.png', '/manifest.webmanifest', '/offline.html'].sort(),
    );
  });

  test('/sw.js is served with a Cache-Control: no-cache header', async ({ page }) => {
    const response = await page.goto('/sw.js');
    expect(response?.headers()['cache-control']).toBe('no-cache');
  });
});

test.describe('offline behaviour', () => {
  test('a navigation while offline shows the static offline page, never a stale copy of real content', async ({
    page,
    context,
  }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));

    await context.setOffline(true);
    await page.goto('/').catch(() => {
      // A hard navigation attempt while offline may itself reject
      // depending on browser/Playwright version -- the assertion below is
      // what actually matters: the page ends up showing offline.html's
      // content either way, since the service worker's own fetch handler
      // serves it as the navigation's response.
    });

    await expect(page.getByRole('heading', { name: 'You’re offline' })).toBeVisible();
    // The critical negative assertion: no fragment of real, previously-
    // viewed line-status content (e.g. this app's nav bar) is present --
    // this is Decision 1's central safety property, exercised end to end.
    await expect(page.getByRole('navigation')).toHaveCount(0);

    await context.setOffline(false);
  });

  test('a mutation request still fails normally offline, never served from any cache', async ({ page, context }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));

    await context.setOffline(true);
    const outcome = await page.evaluate(async () => {
      try {
        await fetch('/api/preferences', { method: 'GET' });
        return 'unexpected-success';
      } catch {
        return 'failed-as-expected';
      }
    });
    expect(outcome).toBe('failed-as-expected');

    await context.setOffline(false);
  });
});
```

- [ ] **Step 2: Run the new suite**

Run: `cd frontend && npm run test:e2e -- service-worker.spec.ts`
Expected: PASS, all 5 tests. If the offline-navigation test is flaky on first run (service worker activation timing), re-run once — `page.waitForFunction(() => navigator.serviceWorker.ready...)` should already guard against this, but this is the plan's one genuinely browser-timing-sensitive test, named honestly rather than hidden.

- [ ] **Step 3: Commit**

```bash
git add frontend/e2e/service-worker.spec.ts
git commit -m "Add e2e coverage: SW registration, precaching, offline navigation, offline mutation failure"
```

---

### Task 8: Final verification

**Files:** none (verification only).

**Depends on:** every other task.

- [ ] **Step 1: Full Vitest suite**

Run: `cd frontend && npx vitest run`
Expected: PASS, no regressions anywhere in the suite (this plan's own new test files included).

- [ ] **Step 2: TypeScript check**

Run: `cd frontend && npx tsc --noEmit`
Expected: PASS. (`public/*.js` files are outside `tsconfig.json`'s `include` — only `**/*.ts`/`**/*.tsx` — so `sw.js`/`sw-cache-rules.js` are never type-checked; this is expected, not an oversight.)

- [ ] **Step 3: Production build, including the stamping step**

Run:

```bash
cd frontend
npm run build
cat public/sw.js | grep CACHE_NAME
git checkout -- public/sw.js
```

Expected: build succeeds; `CACHE_NAME` shows a real build id, not `__BUILD_ID__`; the final `git checkout` restores the placeholder so the working tree stays clean.

- [ ] **Step 4: Docker image sanity check — the full `public/` directory, not just the two icons Task 1 checked**

Run from the repo root:

```bash
docker build -f frontend/Dockerfile --target runtime-prod -t distant-signal-frontend-test .
docker run --rm --entrypoint ls distant-signal-frontend-test -la public/
```

Expected: `icon-192.png`, `icon-512.png`, `manifest.webmanifest` is not a static file here (it's generated by `app/manifest.ts`, served dynamically, not present under `public/`), `offline.html`, `sw.js`, `sw-cache-rules.js` are all listed.

- [ ] **Step 5: Full e2e suite**

Run: `cd frontend && npm run test:e2e`
Expected: PASS, including every pre-existing test in `frontend/e2e/` (none, before this plan) and this plan's new `service-worker.spec.ts`.

- [ ] **Step 6: Confirm no unintended diff remains**

Run: `git status`
Expected: clean (no stray stamped `sw.js`, no other uncommitted changes) other than this plan's own commits already made in Tasks 1-7.
