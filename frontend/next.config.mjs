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

// This app keeps a billable Anthropic API key and MCP OAuth tokens in
// localStorage (lib/anthropicKey.ts, lib/mcpOAuthProvider.ts) -- a
// deliberate, documented product decision (see those files' own doc
// comments), unchanged by this CSP. What this closes is the gap next to
// it: with no Content-Security-Policy at all, a future XSS (a DOMPurify
// bypass in app/incidents/[id]/page.tsx's `dangerouslySetInnerHTML`, a
// compromised dependency) could read those credentials and `fetch`/
// `sendBeacon`/`<img src=...>` them to any origin on the internet with
// nothing in the browser to stop it. This constrains which origins a
// request FROM this page can ever reach, so that same script is confined
// to the handful of origins this app already trusts with those
// credentials by design.
function railMcpOrigin() {
  const url = process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL;
  if (!url) return null;
  try {
    return new URL(url).origin;
  } catch {
    // Malformed env var -- degrade to omitting it rather than crash the
    // whole config module over a broken deploy-time value; ChatPanel.tsx's
    // own MCP call would fail loudly on its own in that case anyway.
    return null;
  }
}

// `connect-src` is the directive doing most of the exfiltration-blocking
// work here, but NOT all of it -- see the "what this CSP does not cover"
// paragraph below, added for the 2026-09-26 "Repeater Signal" review's
// finding L7 (the previous copy of this comment overstated what
// `connect-src`/`form-action` together actually confine). `connect-src`
// itself: 'self' (this app's own same-origin `/api/*` proxy calls), the
// Anthropic API (components/ChatPanel.tsx's own direct
// `new Anthropic({ dangerouslyAllowBrowser: true })` call -- the entire
// reason the API key lives in the browser at all), and this deployment's
// own `NEXT_PUBLIC_RAILMCP_PUBLIC_URL` origin (ChatPanel.tsx's MCP
// `StreamableHTTPClientTransport`, a direct browser-to-railMcp connection
// that carries the MCP OAuth token). Grepped for every external
// `src=`/`href=` this app's own components render before writing this --
// the only one found (components/OpenDataAttribution.tsx's plain link to
// nationalrail.co.uk) is a normal, user-initiated navigation, not a
// same-page fetch, so it needs no `connect-src` entry; there is no Google
// Fonts or other CDN usage anywhere in the codebase to allow for either.
//
// What this CSP does NOT cover: `connect-src` governs `fetch`/`XHR`/
// `sendBeacon`/`WebSocket`/`<img>`-style subresource requests a script
// makes FROM this page -- it does not restrict a TOP-LEVEL navigation a
// script itself triggers (`location.href = ...`, `location.assign(...)`,
// `window.open(...)`, or synthesizing and clicking an `<a>`). A script that
// got past `script-src` could still exfiltrate `localStorage` by simply
// navigating (or opening a new tab to) `https://evil.example/#<secret>` --
// nothing in the directives below stops that. `form-action 'self'` (kept,
// below) is often mistaken for covering this too, but it only restricts
// where an HTML `<form>` may submit; it has no effect on a script-driven
// navigation that never goes through form submission at all. The CSP
// working group did once draft a `navigate-to` directive for exactly this
// gap, but it was dropped from the spec and ships in no current browser --
// there is currently no standard CSP directive this app (or any app) can
// add to close it. Closing it for real needs something CSP itself can't
// provide -- e.g. Trusted Types' extended sink coverage for
// `Location`/`Window.open` -- which needs a real Trusted Types policy
// defined app-wide first (today's `dangerouslySetInnerHTML` use in
// app/incidents/[id]/page.tsx would need one anyway); out of scope for this
// pass, which only tightens what a static `headers()` entry can express.
//
// `frame-src 'none'`/`worker-src 'self'` (added alongside this comment
// correction): this app renders no `<iframe>` anywhere (grepped for one
// before adding this) and only ever loads one worker script, same-origin
// (`public/sw.js`, registered by `components/ServiceWorkerRegister.tsx`) --
// neither needs to fall back to `default-src 'self'` implicitly, so stating
// them explicitly removes any dependence on cross-browser fallback-chain
// behavior for two directives that are cheap to pin down precisely.
//
// `script-src`/`style-src` both need 'unsafe-inline', noted here as the
// two directives most worth a follow-up review:
//   - Mantine's `<ColorSchemeScript>` (app/layout.tsx) renders a small
//     inline `<script>` that stamps `data-mantine-color-scheme` before
//     hydration, to avoid a flash of the wrong theme -- Mantine's own
//     documented pattern, normally paired with a per-request `nonce` prop.
//     That needs a `middleware.ts` minting a fresh nonce per request (this
//     app has none today) -- out of scope for this pass, which only adds a
//     static header via `headers()` below. This directive alone WOULD let
//     an injected inline `<script>` execute, same as having no script-src
//     at all -- but on its own it does not help exfiltrate anything via a
//     `fetch`/`XHR`/`sendBeacon`/websocket/`<img>` request; the
//     `connect-src`/`img-src` restrictions here still confine where that
//     KIND of request can go (a top-level navigation is a separate matter
//     -- see the paragraph above).
//   - This app (and Mantine's own components) render plain React inline
//     `style={{...}}` throughout -- `style-src 'self'` alone blocks every
//     one of those under a browser that enforces CSP on the `style`
//     attribute, which would visibly break the app today. Inline-style
//     injection has no plain CSS-only way to read localStorage and send it
//     anywhere, so this is the low-risk, near-universal relaxation almost
//     every React app's CSP carries.
function contentSecurityPolicy() {
  const mcpOrigin = railMcpOrigin();
  const connectSrc = ["'self'", 'https://api.anthropic.com', ...(mcpOrigin ? [mcpOrigin] : [])].join(' ');
  return [
    "default-src 'self'",
    "script-src 'self' 'unsafe-inline'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self'",
    `connect-src ${connectSrc}`,
    "frame-src 'none'",
    "worker-src 'self'",
    "frame-ancestors 'none'",
    "form-action 'self'",
    "base-uri 'self'",
    "object-src 'none'",
  ].join('; ');
}

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
      // Every route, page and API alike -- Next merges headers from every
      // matching entry, so this adds to (never replaces) the /sw.js rule
      // above for that one path. A CSP header on a JSON `/api/*` response
      // is inert (browsers only ever enforce it on a response that becomes
      // a Document), so this is harmless there and correct everywhere a
      // real page is served, including `/connect-claude/authorize`'s own
      // bare-HTML consent screen (app/connect-claude/authorize/route.ts),
      // which renders one untrusted, if escaped, value into its markup.
      {
        source: '/:path*',
        headers: [{ key: 'Content-Security-Policy', value: contentSecurityPolicy() }],
      },
    ];
  },
};

export default nextConfig;
