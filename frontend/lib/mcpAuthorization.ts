import { auth, type AuthResult } from '@modelcontextprotocol/sdk/client/auth.js';
import { BrowserMcpOAuthProvider } from './mcpOAuthProvider';

/** The chat's MCP endpoint -- the same URL `ChatPanel` hands
 * `StreamableHTTPClientTransport`, so a sign-in started here asks for the
 * same resource the transport's own reauth-on-401 would. */
export function mcpEndpointUrl(mcpServerUrl: string): string {
  return `${mcpServerUrl.replace(/\/+$/, '')}/mcp`;
}

export function chatOAuthProvider(): BrowserMcpOAuthProvider {
  return new BrowserMcpOAuthProvider(`${window.location.origin}/chat/callback`);
}

/** "Connect"/"Reconnect": forget everything this browser holds for the MCP
 * server -- registration, tokens, verifier, pending state -- and start the
 * sign-in from scratch. Dropping the registration too is deliberate: the
 * visitor usually gets here because something about the old one stopped
 * working (e.g. the server expired it, which strands the browser on a 400
 * at `/authorize` rather than bringing it back), and a fresh Dynamic Client
 * Registration costs one request. Resolves `'REDIRECT'` once the browser is
 * on its way to the authorization server. */
export async function startMcpSignIn(
  mcpServerUrl: string,
  provider: BrowserMcpOAuthProvider = chatOAuthProvider(),
): Promise<AuthResult> {
  provider.invalidateCredentials('all');
  return auth(provider, { serverUrl: mcpEndpointUrl(mcpServerUrl) });
}
