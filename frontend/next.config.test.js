import { describe, it, expect, afterEach } from 'vitest';
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
describe('next.config.mjs Content-Security-Policy header', () => {
  afterEach(() => {
    delete process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL;
  });

  async function cspValue() {
    const entries = await nextConfig.headers();
    const match = entries.find((entry) => entry.source === '/:path*');
    const header = match.headers.find((h) => h.key === 'Content-Security-Policy');
    return header.value;
  }

  it('still carries the pre-existing /sw.js no-cache rule (regression)', async () => {
    const entries = await nextConfig.headers();
    const swEntry = entries.find((entry) => entry.source === '/sw.js');
    expect(swEntry.headers).toEqual([{ key: 'Cache-Control', value: 'no-cache' }]);
  });

  it('restricts default-src, script-src and connect-src to self plus the Anthropic API', async () => {
    const csp = await cspValue();
    expect(csp).toContain("default-src 'self'");
    expect(csp).toContain("script-src 'self' 'unsafe-inline'");
    expect(csp).toContain("connect-src 'self' https://api.anthropic.com");
  });

  it('never allows a wildcard or unrestricted source anywhere in the policy', async () => {
    const csp = await cspValue();
    expect(csp).not.toContain('*');
  });

  it('blocks framing and restricts form submission/base URI to self', async () => {
    const csp = await cspValue();
    expect(csp).toContain("frame-ancestors 'none'");
    expect(csp).toContain("form-action 'self'");
    expect(csp).toContain("base-uri 'self'");
    expect(csp).toContain("object-src 'none'");
  });

  // Finding L7 of the 2026-09-26 "Repeater Signal" review: this app renders
  // no `<iframe>` anywhere and only ever loads one same-origin worker script
  // (`public/sw.js`), so both can be pinned down explicitly instead of
  // relying on the `default-src 'self'` fallback.
  it('blocks framing this page embeds and restricts workers to self', async () => {
    const csp = await cspValue();
    expect(csp).toContain("frame-src 'none'");
    expect(csp).toContain("worker-src 'self'");
  });

  it("adds this deployment's own NEXT_PUBLIC_RAILMCP_PUBLIC_URL origin to connect-src when configured", async () => {
    process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL = 'https://railmcp.example.com/some/path';
    const csp = await cspValue();
    expect(csp).toContain("connect-src 'self' https://api.anthropic.com https://railmcp.example.com");
  });

  it('omits the railMcp origin from connect-src rather than crashing when the env var is unset', async () => {
    delete process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL;
    const csp = await cspValue();
    expect(csp).toContain("connect-src 'self' https://api.anthropic.com;");
  });

  it('omits the railMcp origin from connect-src rather than crashing when the env var is malformed', async () => {
    process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL = 'not-a-valid-url';
    const csp = await cspValue();
    expect(csp).toContain("connect-src 'self' https://api.anthropic.com;");
  });
});
