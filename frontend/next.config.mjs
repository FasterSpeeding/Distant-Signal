// Origins `next dev` accepts cross-origin dev requests (HMR, the dev
// overlay, /_next/* assets) from. Supplied by docker-compose.dev.yml from
// NEXT_ALLOWED_DEV_ORIGINS in `dev.env` — comma-separated — rather than
// hardcoded, because the value that Next asks for is a Compose bridge IP
// that changes every time the network is recreated.
//
// Empty (the default) means the key is omitted from the config entirely,
// which is what you want for plain http://localhost:3000 browsing; setting
// `allowedDevOrigins: []` is not the same thing.
const devOrigins = (process.env.NEXT_ALLOWED_DEV_ORIGINS ?? "")
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);

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
