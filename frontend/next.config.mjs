import { computeBuildId } from './scripts/build-id.mjs';

// Origins `next dev` accepts cross-origin dev requests (HMR, the dev
// overlay, /_next/* assets) from. Supplied by docker-compose.dev.yml from
// NEXT_ALLOWED_DEV_ORIGINS in `dev.env` — comma-separated — rather than
// hardcoded, because the value that Next asks for is a Compose bridge IP
// that changes every time the network is recreated.
//
// Empty (the default) means the key is omitted from the config entirely,
// which is what you want for plain http://localhost:3000 browsing; setting
// `allowedDevOrigins: []` is not the same thing.
const devOrigins = (process.env.NEXT_ALLOWED_DEV_ORIGINS ?? '')
  .split(',')
  .map((s) => s.trim())
  .filter(Boolean);

// The page Content-Security-Policy is built per request, with a nonce, by
// proxy.ts (see lib/csp.ts for the policy and why). This file only covers
// the paths proxy.ts's matcher skips that still need a policy of their own.
// Next merges headers from every matching `headers()` entry, so none of
// these sources may overlap a path proxy.ts handles: two CSP headers are
// both enforced, and a nonce-less one would block the page's scripts.
//
// Shared by the static policies below. Same non-script directives as
// lib/csp.ts's page policy, minus the Anthropic/railMcp `connect-src`
// entries: neither the service worker nor the offline page talks to them.
const STATIC_BASE_DIRECTIVES = [
  "default-src 'self'",
  "img-src 'self' data:",
  "font-src 'self'",
  "connect-src 'self'",
  "frame-src 'none'",
  "worker-src 'self'",
  "frame-ancestors 'none'",
  "form-action 'self'",
  "base-uri 'self'",
  "object-src 'none'",
];

// `/sw.js` (and the `/sw-cache-rules.js` it `importScripts`): a service
// worker's own response CSP governs the worker, including its `fetch()`es.
// It runs no inline script.
const SERVICE_WORKER_CSP = [...STATIC_BASE_DIRECTIVES, "script-src 'self'", "style-src 'self'"].join('; ');

// `/offline.html`: a static file the service worker serves when a
// navigation fails. It has an inline `<script>` and an inline `onclick`, and
// no nonce can be stamped on a static file, so it keeps `'unsafe-inline'`.
// It renders no request-derived content, so nothing can be injected into it.
const OFFLINE_PAGE_CSP = [
  ...STATIC_BASE_DIRECTIVES,
  "script-src 'self' 'unsafe-inline'",
  "style-src 'self' 'unsafe-inline'",
].join('; ');

// `/api/*`: the backend proxy. It returns JSON and redirects, never a page,
// so if a response were ever rendered as a document it should be able to do
// nothing at all.
const API_CSP = ["default-src 'none'", "frame-ancestors 'none'", "base-uri 'none'", "form-action 'none'"].join('; ');

/** @type {import('next').NextConfig} */
const nextConfig = {
  // Emits `.next/standalone` -- a traced, minimal dependency tree (only
  // what the built app actually imports, not the full `npm ci` output)
  // plus a self-contained `server.js` entrypoint -- so frontend/Dockerfile's
  // runtime-prod stage can ship a much smaller final image than copying
  // the whole node_modules directory in. Two things this mode does NOT
  // bundle automatically (both documented Next.js gotchas): `.next/static`
  // and `public/` -- the Dockerfile copies both in manually alongside
  // `.next/standalone`. No custom server, no monorepo/workspace root here,
  // so none of `output: 'standalone'`'s other edge cases (custom
  // `outputFileTracingRoot`, workspace-relative tracing) apply.
  output: 'standalone',
  // A hash of the frontend's inputs (scripts/build-id.mjs), not Next's
  // random default: the build ID is stamped into public/sw.js, so a random
  // one made every deploy install a new service worker and drop its
  // caches, and made every image build differ. Now both change only when
  // frontend/ does.
  generateBuildId: () => computeBuildId(import.meta.dirname),
  ...(devOrigins.length ? { allowedDevOrigins: devOrigins } : {}),
  // The Journeys page (`app/journeys/page.tsx`) was the Track page at
  // `/track/mine` until DS-115 (21 Oct 2026), and before that
  // `/track/tickets` was a tickets list of its own, until Part B of the
  // upload-first ticket-tracking plan merged it into `/track/mine`.
  // Config-level permanent redirects (not rendered stub pages) keep
  // bookmarks and shared links to either old path working. Both go straight
  // to `/journeys`, never through each other, so no visitor takes two hops.
  // `/track/mine/add-ticket` keeps its URL: a `source` with no wildcard
  // matches its one path only.
  // eslint-disable-next-line @typescript-eslint/require-await -- NextConfig types redirects() as returning a Promise
  async redirects() {
    return [
      {
        source: '/track/mine',
        destination: '/journeys',
        permanent: true,
      },
      {
        source: '/track/tickets',
        destination: '/journeys',
        permanent: true,
      },
      // "New journey" (`/journeys/new`) merged into "Plan a journey"
      // (`/plan`) in DS-194. Next passes the query through unchanged, and
      // `/plan` reads every parameter the old page did (`?mode=`,
      // `?fromTflStop=`), so old links and bookmarks open the same option.
      {
        source: '/journeys/new',
        destination: '/plan',
        permanent: true,
      },
      // Train pushes linked to `/track/{id}`, a page that never existed,
      // until 22 Oct 2026; this keeps the links already sent working.
      {
        source: '/track/:trackingId(\\d+)',
        destination: '/train/by-id/:trackingId',
        permanent: true,
      },
    ];
  },
  // /sw.js's own byte content changes whenever the frontend does (scripts/
  // stamp-sw-version.mjs stamps the BUILD_ID into it) -- an
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
  // eslint-disable-next-line @typescript-eslint/require-await -- NextConfig types headers() as returning a Promise
  async headers() {
    return [
      {
        source: '/sw.js',
        headers: [
          { key: 'Cache-Control', value: 'no-cache' },
          { key: 'Content-Security-Policy', value: SERVICE_WORKER_CSP },
        ],
      },
      // Keep in sync with proxy.ts's matcher: these are the paths it skips.
      {
        source: '/sw-cache-rules.js',
        headers: [{ key: 'Content-Security-Policy', value: SERVICE_WORKER_CSP }],
      },
      {
        source: '/offline.html',
        headers: [{ key: 'Content-Security-Policy', value: OFFLINE_PAGE_CSP }],
      },
      {
        source: '/api/:path*',
        headers: [{ key: 'Content-Security-Policy', value: API_CSP }],
      },
    ];
  },
};

export default nextConfig;
