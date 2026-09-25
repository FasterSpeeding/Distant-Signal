import { describe, it, expect, beforeEach } from 'vitest';
import { BrowserMcpOAuthProvider } from './mcpOAuthProvider';

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
    provider.saveClientInformation(info as never);
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
});
