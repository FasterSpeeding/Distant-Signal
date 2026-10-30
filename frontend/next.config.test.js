import { describe, it, expect } from 'vitest';
import nextConfig from './next.config.mjs';

// Finding 6 of the 2026-09-24 security review: this app kept a billable
// Anthropic API key and MCP OAuth tokens in localStorage
// (lib/anthropicKey.ts, lib/mcpOAuthProvider.ts) with NO
// Content-Security-Policy on the page at all -- a future XSS could read
// them and exfiltrate to any origin. `next.config.mjs`'s `headers()` now
// adds one; this is a plain `.js` test file (not `.ts`) deliberately --
// `tsconfig.json` has `allowJs: false` and no declaration for
// `next.config.mjs`, so a `.ts` test importing it directly wouldn't
// resolve under `tsc --noEmit`, and `.js`/`.mjs` files are outside that
// config's own `include` globs anyway (only `**/*.ts`/`**/*.tsx`).
// Vitest's `include` glob does cover plain `.js` test files, unlike
// `.mjs` ones -- see vitest.config.ts's own comment on why `.spec.ts` was
// excluded; the same glob (`**/*.test.{ts,tsx,js,jsx}`) is why this file
// is named `.test.js`, not `.test.mjs`.
//
// Since Repeater Signal L7 the page policy is per request (proxy.ts,
// lib/csp.ts, tested in lib/csp.test.ts and proxy.test.ts). What stays here
// is the static policy for the paths proxy.ts skips.
describe('next.config.mjs static Content-Security-Policy headers', () => {
  // `headers` is optional on NextConfig; this config always defines it.
  async function headerEntries() {
    if (!nextConfig.headers) throw new Error('next.config.mjs defines no headers()');
    return nextConfig.headers();
  }

  /** @param {string} source */
  async function cspFor(source) {
    const entries = await headerEntries();
    const values = entries
      .filter((entry) => entry.source === source)
      .flatMap((entry) => entry.headers)
      .filter((h) => h.key === 'Content-Security-Policy')
      .map((h) => h.value);
    expect(values).toHaveLength(1);
    return values[0];
  }

  it('still carries the /sw.js no-cache rule (regression)', async () => {
    const entries = await headerEntries();
    const sw = entries.find((entry) => entry.source === '/sw.js');
    expect(sw?.headers).toContainEqual({
      key: 'Content-Security-Policy',
      value: /** @type {unknown} */ (expect.any(String)),
    });
    expect(sw?.headers).toContainEqual({ key: 'Cache-Control', value: 'no-cache' });
  });

  it('no longer sets a catch-all CSP, which would stack a nonce-less policy on every page', async () => {
    const entries = await headerEntries();
    const sources = entries
      .filter((entry) => entry.headers.some((h) => h.key === 'Content-Security-Policy'))
      .map((entry) => entry.source)
      .sort();
    expect(sources).toEqual(['/api/:path*', '/offline.html', '/sw-cache-rules.js', '/sw.js']);
  });

  it('gives the service worker scripts no inline script', async () => {
    for (const source of ['/sw.js', '/sw-cache-rules.js']) {
      const csp = await cspFor(source);
      expect(csp).toContain("script-src 'self'");
      expect(csp).not.toContain('unsafe-inline');
      expect(csp).toContain("connect-src 'self'");
      expect(csp).toContain("frame-ancestors 'none'");
      expect(csp).not.toContain('*');
    }
  });

  it("keeps 'unsafe-inline' only for the static offline page, which renders nothing request-derived", async () => {
    const csp = await cspFor('/offline.html');
    expect(csp).toContain("script-src 'self' 'unsafe-inline'");
    expect(csp).toContain("connect-src 'self'");
    expect(csp).toContain("object-src 'none'");
    expect(csp).not.toContain('*');
  });

  it('locks /api/* responses down completely', async () => {
    const csp = await cspFor('/api/:path*');
    expect(csp).toContain("default-src 'none'");
    expect(csp).toContain("frame-ancestors 'none'");
  });
});

describe('next.config.mjs redirects', () => {
  // DS-194: New journey merged into Plan a journey. No query in the
  // destination, so Next passes the old URL's query (`?mode=`,
  // `?fromTflStop=`) through untouched.
  it('sends /journeys/new to /plan for good, keeping its query', async () => {
    if (!nextConfig.redirects) throw new Error('next.config.mjs defines no redirects()');
    const entries = await nextConfig.redirects();
    expect(entries.filter((entry) => entry.source.startsWith('/journeys/new'))).toEqual([
      { source: '/journeys/new', destination: '/plan', permanent: true },
    ]);
    expect(entries.filter((entry) => entry.source.startsWith('/plan'))).toEqual([]);
  });

  it.each(['/track/mine', '/track/tickets'])(
    'sends the old %s page to /journeys for good, in one hop',
    async (path) => {
      if (!nextConfig.redirects) throw new Error('next.config.mjs defines no redirects()');
      const entries = await nextConfig.redirects();
      expect(entries.filter((entry) => entry.source === path)).toEqual([
        { source: path, destination: '/journeys', permanent: true },
      ]);
    },
  );

  it('leaves /track/mine/add-ticket and /journeys itself unredirected', async () => {
    if (!nextConfig.redirects) throw new Error('next.config.mjs defines no redirects()');
    const entries = await nextConfig.redirects();
    const sources = entries.map((entry) => entry.source);
    expect(sources).not.toContain('/track/mine/add-ticket');
    expect(sources).not.toContain('/journeys');
    // The one parameterised source, for links sent before 22 Oct 2026, takes
    // digits only, so it can't catch `/track/mine` or anything under it.
    const parameterised = sources.filter((source) => source.includes(':') || source.includes('*'));
    expect(parameterised).toEqual(['/track/:trackingId(\\d+)']);
  });

  it('sends the old numeric /track/{id} push links to the train page', async () => {
    if (!nextConfig.redirects) throw new Error('next.config.mjs defines no redirects()');
    const entries = await nextConfig.redirects();
    expect(entries).toContainEqual({
      source: '/track/:trackingId(\\d+)',
      destination: '/train/by-id/:trackingId',
      permanent: true,
    });
  });
});
