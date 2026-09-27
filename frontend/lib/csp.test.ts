import { describe, it, expect } from 'vitest';
import { buildContentSecurityPolicy, generateNonce, railMcpOrigin, runtimeRailMcpPublicUrl } from './csp';

function directive(csp: string, name: string): string {
  const found = csp.split('; ').find((d) => d === name || d.startsWith(`${name} `));
  if (found === undefined) throw new Error(`no ${name} in ${csp}`);
  return found;
}

describe('buildContentSecurityPolicy', () => {
  it('allows scripts only by nonce and strict-dynamic, never unsafe-inline (Repeater Signal L7)', () => {
    const csp = buildContentSecurityPolicy({ nonce: 'abc123==' });
    expect(directive(csp, 'script-src')).toBe("script-src 'self' 'nonce-abc123==' 'strict-dynamic'");
    expect(directive(csp, 'script-src')).not.toContain('unsafe-inline');
    expect(csp).not.toContain('unsafe-eval');
  });

  it("adds 'unsafe-eval' in dev only", () => {
    const csp = buildContentSecurityPolicy({ nonce: 'n', dev: true });
    expect(directive(csp, 'script-src')).toBe("script-src 'self' 'nonce-n' 'strict-dynamic' 'unsafe-eval'");
  });

  it('keeps the rest of the previous static policy', () => {
    const csp = buildContentSecurityPolicy({ nonce: 'n' });
    expect(csp.split('; ')).toEqual([
      "default-src 'self'",
      "script-src 'self' 'nonce-n' 'strict-dynamic'",
      "style-src 'self' 'unsafe-inline'",
      "img-src 'self' data:",
      "font-src 'self'",
      "connect-src 'self' https://api.anthropic.com",
      "frame-src 'none'",
      "worker-src 'self'",
      "frame-ancestors 'none'",
      "form-action 'self'",
      "base-uri 'self'",
      "object-src 'none'",
    ]);
    expect(csp).not.toContain('*');
  });

  it("adds the railMcp origin to connect-src when configured (FE-2)", () => {
    const csp = buildContentSecurityPolicy({ nonce: 'n', railMcpPublicUrl: 'https://railmcp.example.com/some/path' });
    expect(directive(csp, 'connect-src')).toBe("connect-src 'self' https://api.anthropic.com https://railmcp.example.com");
  });

  it('omits a malformed railMcp URL rather than throwing', () => {
    const csp = buildContentSecurityPolicy({ nonce: 'n', railMcpPublicUrl: 'not-a-valid-url' });
    expect(directive(csp, 'connect-src')).toBe("connect-src 'self' https://api.anthropic.com");
  });

  it('refuses a nonce that could break out of the header', () => {
    expect(() => buildContentSecurityPolicy({ nonce: "x'; script-src *" })).toThrow();
    expect(() => buildContentSecurityPolicy({ nonce: '' })).toThrow();
  });
});

describe('generateNonce', () => {
  it('is base64 of 16 bytes and differs per call', () => {
    const a = generateNonce();
    const b = generateNonce();
    expect(a).toMatch(/^[A-Za-z0-9+/]{22}==$/);
    expect(a).not.toBe(b);
    // Usable in the policy.
    expect(buildContentSecurityPolicy({ nonce: a })).toContain(`'nonce-${a}'`);
  });
});

describe('railMcpOrigin', () => {
  it('returns the origin, or null when unset, malformed or opaque', () => {
    expect(railMcpOrigin('https://mcp.example.com:8443/mcp')).toBe('https://mcp.example.com:8443');
    expect(railMcpOrigin(undefined)).toBeNull();
    expect(railMcpOrigin('')).toBeNull();
    expect(railMcpOrigin('nope')).toBeNull();
    expect(railMcpOrigin('foo:bar')).toBeNull();
  });
});

describe('runtimeRailMcpPublicUrl', () => {
  it('reads NEXT_PUBLIC_RAILMCP_PUBLIC_URL from the given environment', () => {
    expect(runtimeRailMcpPublicUrl({ NEXT_PUBLIC_RAILMCP_PUBLIC_URL: 'https://m.example' })).toBe(
      'https://m.example',
    );
    expect(runtimeRailMcpPublicUrl({})).toBeUndefined();
  });
});
