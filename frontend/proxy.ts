import { NextResponse, type NextRequest } from 'next/server';
import { buildContentSecurityPolicy, generateNonce, NONCE_HEADER, runtimeRailMcpPublicUrl } from '@/lib/csp';

// Mints a per-request CSP nonce, following Next's documented pattern
// (node_modules/next/dist/docs/01-app/02-guides/content-security-policy.md).
// See lib/csp.ts for the policy itself.
//
// The policy goes on the REQUEST headers as well as the response: Next reads
// the nonce out of the request's `Content-Security-Policy` header while
// rendering and stamps it onto its own scripts. `x-nonce` is how the root
// layout reads it (for Mantine's `<ColorSchemeScript>`). Both are set, not
// appended, so a client-supplied value is always overwritten.
//
// Reading the request headers makes every page dynamically rendered: a
// nonce is per request, so no HTML can be prerendered at build time.
export function proxy(request: NextRequest) {
  const nonce = generateNonce();
  const csp = buildContentSecurityPolicy({
    nonce,
    railMcpPublicUrl: runtimeRailMcpPublicUrl(),
    dev: process.env.NODE_ENV === 'development',
  });

  const requestHeaders = new Headers(request.headers);
  requestHeaders.set(NONCE_HEADER, nonce);
  requestHeaders.set('Content-Security-Policy', csp);

  const response = NextResponse.next({ request: { headers: requestHeaders } });
  response.headers.set('Content-Security-Policy', csp);
  return response;
}

// Pages only. Skipped:
// - `/api/*`: the backend proxy returns JSON; no nonce needed, and not paying
//   the proxy hop on every API call;
// - `/_next/static`, `/_next/image`: build assets;
// - the root-level static files (`/robots.txt`, `/sw.js` and its
//   `/sw-cache-rules.js`, `/offline.html`, the manifest and icons).
//   next.config.mjs gives `/api/*`, the service worker scripts and
//   `/offline.html` static policies of their own;
// - `/healthz`: the probes' plain-text liveness route (app/healthz), which
//   must stay dependency-free and cheap;
// - `next/link` prefetches, as Next's guide recommends. They fetch an RSC
//   payload, not a document, so there is nothing for a nonce to protect.
//
// Keep this list in sync with the CSP entries in next.config.mjs's headers().
export const config = {
  matcher: [
    {
      source:
        '/((?!(?:api|_next/static|_next/image)(?:/|$)|(?:healthz|favicon\\.ico|robots\\.txt|sw\\.js|sw-cache-rules\\.js|offline\\.html|manifest\\.webmanifest|icon\\.svg|apple-icon\\.png|icon-192\\.png|icon-512\\.png)$).*)',
      missing: [
        { type: 'header', key: 'next-router-prefetch' },
        { type: 'header', key: 'purpose', value: 'prefetch' },
      ],
    },
  ],
};
