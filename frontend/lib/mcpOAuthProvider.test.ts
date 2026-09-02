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
});
