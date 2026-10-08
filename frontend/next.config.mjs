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
  // /track/tickets and /track/mine were two separate pages
  // (docs/superpowers/specs/2026-08-31-tickets-list-design.md,
  // docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md) until
  // Part B of the upload-first ticket-tracking plan merged them: once a
  // ticket can exist standalone (Part A), a bare "My Tickets" list sits
  // awkwardly next to a bare "My Tracked Trains" list, so `/track/mine` now
  // renders both. A config-level redirect (not a rendered stub page) keeps
  // any bookmarked/linked `/track/tickets` URL working rather than 404ing,
  // without maintaining a second copy of the merged page's content.
  // eslint-disable-next-line @typescript-eslint/require-await -- NextConfig types redirects() as returning a Promise
  async redirects() {
    return [
      {
        source: '/track/tickets',
        destination: '/track/mine',
        permanent: true,
      },
      // The planner's own page is `/plan` (see app/plan/page.tsx for why a
      // route rather than a mode switch); this keeps the mode-switch URL
      // working for anyone who links to it. Not permanent, so the alias
      // can be dropped later without browsers having cached it.
      {
        source: '/journeys/new',
        has: [{ type: 'query', key: 'mode', value: 'plan' }],
        destination: '/plan',
        permanent: false,
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
