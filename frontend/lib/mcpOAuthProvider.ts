import type { OAuthClientProvider } from '@modelcontextprotocol/sdk/client/auth.js';
import type {
  OAuthClientInformationFull,
  OAuthClientMetadata,
  OAuthTokens,
} from '@modelcontextprotocol/sdk/shared/auth.js';

const STORAGE_PREFIX = 'ds-mcp-oauth:';
const CLIENT_INFO_KEY = `${STORAGE_PREFIX}client-information`;
const TOKENS_KEY = `${STORAGE_PREFIX}tokens`;
const CODE_VERIFIER_KEY = `${STORAGE_PREFIX}code-verifier`;
const OAUTH_STATE_KEY = `${STORAGE_PREFIX}oauth-state`;

/** A per-viewer, browser-local OAuth client against `distant-signal-mcp`'s
 * own OAuth 2.1 authorization server -- the same DCR/PKCE-only public-
 * client shape Claude Desktop already gets from `RailMcpOAuthProvider`,
 * just run inside the browser instead of a native app. Backed by
 * `localStorage`, matching this app's existing precedent
 * (`ThemeToggle.tsx`/`PrideToggle.tsx`) -- see the client-side-tokens
 * design doc's Decision 6 for why `localStorage` over `sessionStorage`/
 * IndexedDB.
 *
 * Implements the MCP SDK's own `OAuthClientProvider` interface
 * (`@modelcontextprotocol/sdk/client/auth.js`) so both the SDK's exported
 * `auth()` orchestrator (Task 8's callback route) and
 * `StreamableHTTPClientTransport`'s own `authProvider` option (Task 10's
 * `ChatPanel.tsx`) can drive it directly -- no hand-rolled redirect/
 * exchange/store sequence needed. */
export class BrowserMcpOAuthProvider implements OAuthClientProvider {
  constructor(private readonly callbackUrl: string) {}

  get redirectUrl(): string {
    return this.callbackUrl;
  }

  get clientMetadata(): OAuthClientMetadata {
    return {
      client_name: 'Distant Signal chat',
      redirect_uris: [this.callbackUrl],
      grant_types: ['authorization_code'],
      response_types: ['code'],
      // PKCE-only public client -- no secret, matching every other MCP
      // client this adapter's DCR (`RailMcpOAuthProvider.registerClient`)
      // ever issues.
      token_endpoint_auth_method: 'none',
    };
  }

  clientInformation(): OAuthClientInformationFull | undefined {
    return readJson<OAuthClientInformationFull>(CLIENT_INFO_KEY);
  }

  saveClientInformation(clientInformation: OAuthClientInformationFull): void {
    localStorage.setItem(CLIENT_INFO_KEY, JSON.stringify(clientInformation));
  }

  tokens(): OAuthTokens | undefined {
    return readJson<OAuthTokens>(TOKENS_KEY);
  }

  saveTokens(tokens: OAuthTokens): void {
    localStorage.setItem(TOKENS_KEY, JSON.stringify(tokens));
  }

  redirectToAuthorization(authorizationUrl: URL): void {
    window.location.href = authorizationUrl.toString();
  }

  saveCodeVerifier(codeVerifier: string): void {
    localStorage.setItem(CODE_VERIFIER_KEY, codeVerifier);
  }

  codeVerifier(): string {
    const verifier = localStorage.getItem(CODE_VERIFIER_KEY);
    if (!verifier) {
      throw new Error('No PKCE code verifier found in localStorage -- the authorization flow was not started from this browser');
    }
    return verifier;
  }

  /** Finding 3 of the deferred fapp Low-severity batch (2026-09-24 security
   * review): `auth()` (`@modelcontextprotocol/sdk/client/auth.js`) calls
   * this optional `OAuthClientProvider.state()` hook when it builds the
   * authorization redirect URL, and includes whatever it returns as the
   * OAuth `state` query parameter sent to `distant-signal-mcp`'s own
   * authorization server. Before this, `BrowserMcpOAuthProvider` didn't
   * implement `state()` at all, so no `state` was ever sent -- defense
   * against a forged/planted authorization code at `/chat/callback` rested
   * entirely on the PKCE verifier mismatching at the token endpoint. A
   * random, single-use value stored here (and consumed by
   * `consumeAndVerifyState` below, called from the callback page before it
   * ever calls `auth()` with the code) gives that callback an independent
   * check: the code must have come back from the authorization redirect
   * *this browser* actually initiated, not one an attacker planted via
   * e.g. a stale/replayed callback URL or a mixed-up multi-tab flow. */
  state(): string {
    const value = crypto.randomUUID();
    localStorage.setItem(OAUTH_STATE_KEY, value);
    return value;
  }

  /** Verifies the `state` query parameter `/chat/callback` received against
   * the value `state()` generated and stored before redirecting to the
   * authorization server, then clears the stored value regardless of the
   * outcome -- single-use, so a stale value can't be replayed to validate a
   * later, unrelated callback. Fails closed: a missing stored value (no
   * flow was ever started from this browser), a `null` received value (the
   * callback URL carried none), or a mismatch are all treated as invalid. */
  consumeAndVerifyState(receivedState: string | null): boolean {
    const expected = localStorage.getItem(OAUTH_STATE_KEY);
    localStorage.removeItem(OAUTH_STATE_KEY);
    return expected !== null && receivedState !== null && receivedState === expected;
  }
}

function readJson<T>(key: string): T | undefined {
  const raw = localStorage.getItem(key);
  if (!raw) return undefined;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return undefined;
  }
}
