import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { auth } from '@modelcontextprotocol/sdk/client/auth.js';
import { BrowserMcpOAuthProvider } from './mcpOAuthProvider';
import { startMcpSignIn } from './mcpAuthorization';
import { mcpEndpointUrl } from './mcpInstallLinks';

/** Drives the REAL MCP SDK `auth()` (1.30.0) against a fake
 * distant-signal-mcp authorization server, to prove the provider's
 * `invalidateCredentials`/expiry handling actually recovers from an expired
 * Dynamic Client Registration -- the server answers a stale `client_id`
 * with `400 {"error":"invalid_client"}` at both /token and /authorize. */

const MCP = 'https://mcp.example.com';
const CALLBACK = 'http://localhost:3000/chat/callback';
const DAY_S = 24 * 60 * 60;

interface Call {
  method: string;
  path: string;
  body: string;
}

function fakeAuthServer(opts: { tokenResponse?: () => Response } = {}) {
  const calls: Call[] = [];
  let registrations = 0;
  const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(input instanceof Request ? input.url : input.toString());
    const method = init?.method ?? 'GET';
    const body = init?.body ? String(init.body) : '';
    calls.push({ method, path: url.pathname, body });
    const json = (status: number, value: unknown) =>
      new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } });
    // Like distant-signal-mcp (the SDK's `mcpAuthMetadataRouter` with
    // `resourceServerUrl` = `<public>/mcp`): path-suffixed only, the bare
    // root document 404s.
    if (url.pathname === '/.well-known/oauth-protected-resource/mcp') {
      return json(200, { resource: `${MCP}/mcp`, authorization_servers: [MCP] });
    }
    if (url.pathname.startsWith('/.well-known/oauth-authorization-server')) {
      return json(200, {
        issuer: MCP,
        authorization_endpoint: `${MCP}/authorize`,
        token_endpoint: `${MCP}/token`,
        registration_endpoint: `${MCP}/register`,
        response_types_supported: ['code'],
        grant_types_supported: ['authorization_code', 'refresh_token'],
        code_challenge_methods_supported: ['S256'],
        token_endpoint_auth_methods_supported: ['none'],
      });
    }
    if (url.pathname === '/register' && method === 'POST') {
      registrations += 1;
      return json(201, {
        ...JSON.parse(body),
        client_id: `fresh-client-${registrations}`,
        client_id_issued_at: Math.floor(Date.now() / 1000),
      });
    }
    if (url.pathname === '/token' && method === 'POST') {
      if (opts.tokenResponse) return opts.tokenResponse();
      return json(400, { error: 'invalid_client', error_description: 'Invalid client_id' });
    }
    return new Response('not found', { status: 404 });
  });
  return { fetchMock, calls };
}

function captureRedirect(provider: BrowserMcpOAuthProvider): { url: () => URL | undefined } {
  let captured: URL | undefined;
  vi.spyOn(provider, 'redirectToAuthorization').mockImplementation((url: URL) => {
    captured = url;
  });
  return { url: () => captured };
}

describe('MCP sign-in recovery against the real SDK auth()', () => {
  beforeEach(() => {
    localStorage.clear();
  });
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('on invalid_client refreshing with a server-expired registration: discards it, re-registers, and re-authorizes', async () => {
    const { fetchMock, calls } = fakeAuthServer();
    const provider = new BrowserMcpOAuthProvider(CALLBACK);
    // Young enough locally, but the server has lost it (e.g. its store was
    // flushed) -- only the server's invalid_client reveals that.
    provider.saveClientInformation({
      client_id: 'expired-client',
      redirect_uris: [CALLBACK],
      client_id_issued_at: Math.floor(Date.now() / 1000) - DAY_S,
    });
    provider.saveTokens({ access_token: 'old', token_type: 'Bearer', refresh_token: 'old-refresh' });
    const redirect = captureRedirect(provider);

    const result = await auth(provider, { serverUrl: `${MCP}/mcp`, fetchFn: fetchMock });

    expect(result).toBe('REDIRECT');
    // The refresh with the dead client_id was attempted once...
    const tokenCalls = calls.filter((c) => c.path === '/token');
    expect(tokenCalls).toHaveLength(1);
    expect(tokenCalls[0].body).toContain('client_id=expired-client');
    // ...then everything was discarded and a new client registered.
    expect(calls.filter((c) => c.path === '/register')).toHaveLength(1);
    expect(provider.clientInformation()?.client_id).toBe('fresh-client-1');
    expect(provider.tokens()).toBeUndefined();
    // The browser is sent to /authorize with the NEW client_id, never the dead one.
    expect(redirect.url()?.pathname).toBe('/authorize');
    expect(redirect.url()?.searchParams.get('client_id')).toBe('fresh-client-1');
  });

  it('re-registers a registration older than the server TTL BEFORE going to /authorize (which would 400)', async () => {
    const { fetchMock, calls } = fakeAuthServer();
    const provider = new BrowserMcpOAuthProvider(CALLBACK);
    localStorage.setItem(
      'ds-mcp-oauth:client-information',
      JSON.stringify({
        client_id: 'expired-client',
        redirect_uris: [CALLBACK],
        client_id_issued_at: Math.floor(Date.now() / 1000) - 30 * DAY_S,
      }),
    );
    provider.saveTokens({ access_token: 'old', token_type: 'Bearer', refresh_token: 'old-refresh' });
    const redirect = captureRedirect(provider);

    const result = await auth(provider, { serverUrl: `${MCP}/mcp`, fetchFn: fetchMock });

    expect(result).toBe('REDIRECT');
    // No doomed refresh with the expired client, straight to DCR.
    expect(calls.filter((c) => c.path === '/token')).toHaveLength(0);
    expect(calls.filter((c) => c.path === '/register')).toHaveLength(1);
    expect(redirect.url()?.searchParams.get('client_id')).toBe('fresh-client-1');
  });

  it('refreshes normally (no re-registration) when the registration is still valid', async () => {
    const { fetchMock, calls } = fakeAuthServer({
      tokenResponse: () =>
        new Response(JSON.stringify({ access_token: 'new', token_type: 'Bearer', refresh_token: 'new-refresh' }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
    });
    const provider = new BrowserMcpOAuthProvider(CALLBACK);
    provider.saveClientInformation({
      client_id: 'good-client',
      redirect_uris: [CALLBACK],
      client_id_issued_at: Math.floor(Date.now() / 1000),
    });
    provider.saveTokens({ access_token: 'old', token_type: 'Bearer', refresh_token: 'old-refresh' });

    expect(await auth(provider, { serverUrl: `${MCP}/mcp`, fetchFn: fetchMock })).toBe('AUTHORIZED');
    expect(calls.filter((c) => c.path === '/register')).toHaveLength(0);
    expect(provider.tokens()?.access_token).toBe('new');
  });

  it('on invalid_client during the callback code exchange: clears the dead registration so Reconnect starts clean', async () => {
    const { fetchMock, calls } = fakeAuthServer();
    const provider = new BrowserMcpOAuthProvider(CALLBACK);
    provider.saveClientInformation({
      client_id: 'expired-client',
      redirect_uris: [CALLBACK],
      client_id_issued_at: Math.floor(Date.now() / 1000),
    });
    provider.saveCodeVerifier('verifier');

    // auth() invalidates 'all' and retries once; with no registration left
    // it cannot exchange a code issued to the old one, so it throws.
    await expect(auth(provider, { serverUrl: MCP, authorizationCode: 'code-1', fetchFn: fetchMock })).rejects.toThrow(
      /client information is required/,
    );
    expect(provider.clientInformation()).toBeUndefined();
    expect(localStorage.getItem('ds-mcp-oauth:code-verifier')).toBeNull();

    // Reconnect: fresh DCR + redirect with the new client.
    vi.stubGlobal('fetch', fetchMock);
    const redirect = captureRedirect(provider);
    expect(await startMcpSignIn(MCP, provider)).toBe('REDIRECT');
    expect(calls.filter((c) => c.path === '/register')).toHaveLength(1);
    expect(redirect.url()?.searchParams.get('client_id')).toBe('fresh-client-1');
  });

  it('startMcpSignIn discards every stored credential, even a still-young registration, before signing in', async () => {
    const { fetchMock, calls } = fakeAuthServer();
    vi.stubGlobal('fetch', fetchMock);
    const provider = new BrowserMcpOAuthProvider(CALLBACK);
    provider.saveClientInformation({
      client_id: 'maybe-dead-client',
      redirect_uris: [CALLBACK],
      client_id_issued_at: Math.floor(Date.now() / 1000),
    });
    provider.saveTokens({ access_token: 'old', token_type: 'Bearer' });
    const redirect = captureRedirect(provider);

    expect(await startMcpSignIn(`${MCP}/`, provider)).toBe('REDIRECT');
    expect(calls.filter((c) => c.path === '/register')).toHaveLength(1);
    expect(redirect.url()?.searchParams.get('client_id')).toBe('fresh-client-1');
    expect(redirect.url()?.searchParams.get('state')).toBeTruthy();
    expect(provider.hasAbandonedAuthorization()).toBe(true);
  });

  it('builds the /mcp endpoint URL without a doubled slash', () => {
    expect(mcpEndpointUrl('https://mcp.example.com/')).toBe('https://mcp.example.com/mcp');
    expect(mcpEndpointUrl('https://mcp.example.com')).toBe('https://mcp.example.com/mcp');
  });
});
