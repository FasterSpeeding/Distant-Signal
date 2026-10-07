import { describe, it, expect, beforeEach, vi } from 'vitest';
import { BrowserMcpOAuthProvider, MCP_CLIENT_MAX_AGE_MS, MCP_OAUTH_STORAGE_KEYS } from './mcpOAuthProvider';

describe('BrowserMcpOAuthProvider', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('returns undefined tokens/clientInformation before anything is saved', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    expect(provider.tokens()).toBeUndefined();
    expect(provider.clientInformation()).toBeUndefined();
  });

  it('round-trips tokens through localStorage', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    const tokens = { access_token: 'abc123', token_type: 'Bearer' as const };
    provider.saveTokens(tokens);
    expect(provider.tokens()).toEqual(tokens);

    // A second provider instance reads the same persisted value -- proof
    // this isn't in-memory state, it's actually localStorage-backed.
    const reloaded = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    expect(reloaded.tokens()).toEqual(tokens);
  });

  it('round-trips client information through localStorage', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    const info = { client_id: 'c1', redirect_uris: ['https://status.example.com/chat/callback'] };
    provider.saveClientInformation(info);
    expect(provider.clientInformation()).toEqual(info);
  });

  it('round-trips the PKCE code verifier through localStorage', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    provider.saveCodeVerifier('a-verifier-value');
    expect(provider.codeVerifier()).toBe('a-verifier-value');
  });

  it('throws a clear error reading codeVerifier() before one was saved', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    expect(() => provider.codeVerifier()).toThrow(/no pkce code verifier/i);
  });

  it('exposes the redirect URL and clientMetadata this app registers with', () => {
    const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
    expect(provider.redirectUrl).toBe('https://status.example.com/chat/callback');
    expect(provider.clientMetadata.redirect_uris).toEqual(['https://status.example.com/chat/callback']);
    expect(provider.clientMetadata.token_endpoint_auth_method).toBe('none');
  });

  // Finding 3 of the deferred fapp Low-severity batch (2026-09-24 security
  // review): `state()` is the MCP SDK's own optional hook (`auth()` in
  // `@modelcontextprotocol/sdk/client/auth.js` calls it when starting a new
  // authorization redirect) for supplying an OAuth `state` value -- before
  // this, it wasn't implemented at all, so no `state` was ever sent.
  describe('OAuth state (state() / consumeAndVerifyState())', () => {
    it('generates a fresh, non-empty state value each call and persists the latest one', () => {
      const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
      const first = provider.state();
      expect(first).toBeTruthy();
      const second = provider.state();
      expect(second).toBeTruthy();
      expect(second).not.toBe(first);
      // Only the most recently generated value should verify -- the
      // one actually sent on the authorization redirect that follows.
      expect(provider.consumeAndVerifyState(second)).toBe(true);
    });

    it('verifies a matching state and consumes it (single-use)', () => {
      const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
      const state = provider.state();
      expect(provider.consumeAndVerifyState(state)).toBe(true);
      // Consumed -- replaying the same value against a later callback must
      // not verify again.
      expect(provider.consumeAndVerifyState(state)).toBe(false);
    });

    it('rejects a mismatched state', () => {
      const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
      provider.state();
      expect(provider.consumeAndVerifyState('attacker-planted-state')).toBe(false);
    });

    it('rejects a null received state', () => {
      const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
      provider.state();
      expect(provider.consumeAndVerifyState(null)).toBe(false);
    });

    it('rejects any state when none was ever generated for this browser', () => {
      const provider = new BrowserMcpOAuthProvider('https://status.example.com/chat/callback');
      expect(provider.consumeAndVerifyState('anything')).toBe(false);
    });
  });

  describe('invalidateCredentials()', () => {
    const CALLBACK = 'https://status.example.com/chat/callback';
    const KEYS = {
      client: 'ds-mcp-oauth:client-information',
      savedAt: 'ds-mcp-oauth:client-saved-at',
      tokens: 'ds-mcp-oauth:tokens',
      verifier: 'ds-mcp-oauth:code-verifier',
      state: 'ds-mcp-oauth:oauth-state',
    };

    function seedEverything(provider: BrowserMcpOAuthProvider) {
      provider.saveClientInformation({ client_id: 'c1', redirect_uris: [CALLBACK] });
      provider.saveTokens({ access_token: 'a', token_type: 'Bearer', refresh_token: 'r' });
      provider.saveCodeVerifier('v');
      provider.state();
    }

    function present(): string[] {
      return Object.entries(KEYS)
        .filter(([, key]) => localStorage.getItem(key) !== null)
        .map(([name]) => name)
        .sort();
    }

    it.each([
      ['all', []],
      ['client', ['state', 'tokens', 'verifier']],
      ['tokens', ['client', 'savedAt', 'state', 'verifier']],
      ['verifier', ['client', 'savedAt', 'state', 'tokens']],
      ['discovery', ['client', 'savedAt', 'state', 'tokens', 'verifier']],
    ] as const)('scope %s leaves exactly %j', (scope, remaining) => {
      const provider = new BrowserMcpOAuthProvider(CALLBACK);
      seedEverything(provider);
      expect(present()).toEqual(['client', 'savedAt', 'state', 'tokens', 'verifier']);
      provider.invalidateCredentials(scope);
      expect(present()).toEqual([...remaining]);
    });

    it('covers every key the provider stores under "all"', () => {
      const provider = new BrowserMcpOAuthProvider(CALLBACK);
      seedEverything(provider);
      provider.invalidateCredentials('all');
      for (const key of MCP_OAUTH_STORAGE_KEYS) expect(localStorage.getItem(key)).toBeNull();
    });
  });

  describe('registration expiry (distant-signal-mcp expires DCR clients after 30 days)', () => {
    const CALLBACK = 'https://status.example.com/chat/callback';
    const NOW = Date.UTC(2026, 8, 29);
    const DAY = 24 * 60 * 60 * 1000;

    it('keeps a registration issued less than MCP_CLIENT_MAX_AGE_MS ago', () => {
      const provider = new BrowserMcpOAuthProvider(CALLBACK, () => NOW);
      const info = { client_id: 'c1', redirect_uris: [CALLBACK], client_id_issued_at: (NOW - 28 * DAY) / 1000 };
      provider.saveClientInformation(info);
      expect(provider.clientInformation()).toEqual(info);
    });

    it('drops a registration issued MCP_CLIENT_MAX_AGE_MS or more ago, and the tokens issued to it', () => {
      const provider = new BrowserMcpOAuthProvider(CALLBACK, () => NOW);
      provider.saveClientInformation({
        client_id: 'c1',
        redirect_uris: [CALLBACK],
        client_id_issued_at: (NOW - MCP_CLIENT_MAX_AGE_MS) / 1000,
      });
      provider.saveTokens({ access_token: 'a', token_type: 'Bearer', refresh_token: 'r' });
      expect(provider.clientInformation()).toBeUndefined();
      expect(provider.tokens()).toBeUndefined();
      expect(localStorage.getItem('ds-mcp-oauth:client-information')).toBeNull();
    });

    it('falls back to the time it was saved when the registration has no client_id_issued_at', () => {
      let now = NOW;
      const provider = new BrowserMcpOAuthProvider(CALLBACK, () => now);
      provider.saveClientInformation({ client_id: 'c1', redirect_uris: [CALLBACK] });
      now = NOW + 28 * DAY;
      expect(provider.clientInformation()?.client_id).toBe('c1');
      now = NOW + 29 * DAY;
      expect(provider.clientInformation()).toBeUndefined();
    });

    it('treats a registration with no known age at all as stale', () => {
      localStorage.setItem('ds-mcp-oauth:client-information', JSON.stringify({ client_id: 'legacy' }));
      const provider = new BrowserMcpOAuthProvider(CALLBACK, () => NOW);
      expect(provider.clientInformation()).toBeUndefined();
    });

    it('drops a registration whose client_secret_expires_at has passed', () => {
      const provider = new BrowserMcpOAuthProvider(CALLBACK, () => NOW);
      provider.saveClientInformation({
        client_id: 'c1',
        redirect_uris: [CALLBACK],
        client_id_issued_at: NOW / 1000,
        client_secret_expires_at: (NOW - 1000) / 1000,
      });
      expect(provider.clientInformation()).toBeUndefined();
    });
  });

  describe('abandoned authorization / redirect', () => {
    const CALLBACK = 'https://status.example.com/chat/callback';

    it('reports an authorization whose state was never consumed by the callback', () => {
      const provider = new BrowserMcpOAuthProvider(CALLBACK);
      expect(provider.hasAbandonedAuthorization()).toBe(false);
      const state = provider.state();
      expect(provider.hasAbandonedAuthorization()).toBe(true);
      provider.consumeAndVerifyState(state);
      expect(provider.hasAbandonedAuthorization()).toBe(false);
    });

    it('drops stored (dead) tokens when redirecting to the authorization server', () => {
      const provider = new BrowserMcpOAuthProvider(CALLBACK);
      provider.saveTokens({ access_token: 'a', token_type: 'Bearer' });
      // jsdom can't navigate; it only reports "not implemented".
      const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
      try {
        provider.redirectToAuthorization(new URL('https://mcp.example.com/authorize?x=1'));
      } finally {
        quiet.mockRestore();
      }
      expect(provider.tokens()).toBeUndefined();
    });
  });
});
