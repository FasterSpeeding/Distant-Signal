// The Content-Security-Policy for every rendered page, built per request by
// `proxy.ts` (Repeater Signal L7, and FE-2 of the 2026-09-27 frontend
// review).
//
// Why a per-request policy rather than the static `headers()` entry this
// replaced: this app keeps a billable Anthropic API key and MCP OAuth
// tokens in localStorage (lib/anthropicKey.ts, lib/mcpOAuthProvider.ts), so
// an injected inline `<script>` is the attack that matters. The old
// `script-src 'self' 'unsafe-inline'` let one run. Now `script-src` carries a
// fresh nonce and `'strict-dynamic'`: Next stamps the nonce onto its own
// bootstrap/inline scripts (it parses it out of the request's CSP header,
// which `proxy.ts` sets), the root layout passes it to Mantine's
// `<ColorSchemeScript>`, and scripts those load are trusted transitively.
// An injected `<script>` has no nonce and does not run. Under
// `'strict-dynamic'` CSP3 browsers ignore `'self'`; it stays for older ones.
//
// The railMcp origin in `connect-src` (FE-2) comes from the environment at
// request time. The old static header was evaluated once at `next build`,
// when the production image has no railMcp URL, so the shipped policy never
// allowed the MCP connection even after the chart set the env var.
//
// What this still does NOT cover: `connect-src` restricts `fetch`/XHR/
// `sendBeacon`/WebSocket/subresource requests, not a top-level navigation a
// script triggers (`location.href = ...`, `window.open(...)`). No shipped
// CSP directive can block that (`navigate-to` was dropped from the spec).
// The nonce narrows the ways such a script can get onto the page in the
// first place.
//
// `style-src` keeps `'unsafe-inline'`: Mantine and this app use inline
// `style` attributes and Mantine injects `<style>` tags throughout. Adding a
// nonce there would make browsers ignore `'unsafe-inline'` and break every
// inline style attribute. Injected CSS cannot read localStorage.

/** Header `proxy.ts` uses to hand the nonce to Server Components. */
export const NONCE_HEADER = 'x-nonce';

/** The origin of a railMcp public URL, or null when unset or malformed.
 * Malformed degrades to "omit it" rather than throwing: a broken deploy-time
 * value should not take down every page, and ChatPanel's own MCP call fails
 * loudly on its own in that case. */
export function railMcpOrigin(url: string | undefined): string | null {
  if (!url) return null;
  try {
    const origin = new URL(url).origin;
    // `new URL('foo:bar').origin` is the string "null" for non-special
    // schemes; that is not a usable CSP source.
    return origin === 'null' ? null : origin;
  } catch {
    return null;
  }
}

/** Reads the railMcp public URL from the environment at request time.
 *
 * The chart sets `NEXT_PUBLIC_RAILMCP_PUBLIC_URL` as a runtime env var. A
 * literal `process.env.NEXT_PUBLIC_…` reference is replaced by its
 * build-time value when Next bundles `proxy.ts`, so the name is looked up
 * through a variable to force a real runtime read. */
export function runtimeRailMcpPublicUrl(env: Record<string, string | undefined> = process.env): string | undefined {
  const key = ['NEXT_PUBLIC', 'RAILMCP_PUBLIC_URL'].join('_');
  return env[key];
}

/** A fresh, unguessable nonce: 128 random bits, base64. */
export function generateNonce(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary);
}

export interface CspOptions {
  nonce: string;
  /** The railMcp public URL (any path); only its origin is used. */
  railMcpPublicUrl?: string;
  /** `next dev` needs `'unsafe-eval'` (React's dev-only error-stack
   * reconstruction); production never does. */
  dev?: boolean;
}

export function buildContentSecurityPolicy({ nonce, railMcpPublicUrl, dev = false }: CspOptions): string {
  if (!/^[A-Za-z0-9+/_-]+=*$/.test(nonce)) {
    // Never splice an arbitrary string into the header.
    throw new Error('CSP nonce must be base64');
  }
  const mcpOrigin = railMcpOrigin(railMcpPublicUrl);
  const connectSrc = ["'self'", 'https://api.anthropic.com', ...(mcpOrigin ? [mcpOrigin] : [])];
  const scriptSrc = ["'self'", `'nonce-${nonce}'`, "'strict-dynamic'", ...(dev ? ["'unsafe-eval'"] : [])];
  return [
    "default-src 'self'",
    `script-src ${scriptSrc.join(' ')}`,
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self'",
    `connect-src ${connectSrc.join(' ')}`,
    "frame-src 'none'",
    "worker-src 'self'",
    "frame-ancestors 'none'",
    "form-action 'self'",
    "base-uri 'self'",
    "object-src 'none'",
  ].join('; ');
}
