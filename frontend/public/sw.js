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
