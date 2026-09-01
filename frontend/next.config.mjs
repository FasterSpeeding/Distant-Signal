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
};

export default nextConfig;
