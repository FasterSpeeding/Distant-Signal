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
