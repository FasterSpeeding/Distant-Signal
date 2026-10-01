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
const CLIENT_SAVED_AT_KEY = `${STORAGE_PREFIX}client-saved-at`;

/** `distant-signal-mcp` expires Dynamic Client Registration records 30 days
 * after registration (its `DCR_CLIENT_TTL_SECONDS`, MCP commit eb115ea) and
 * then answers the stale `client_id` with `400 invalid_client` -- at the
 * token endpoint (which `auth()` recovers from via `invalidateCredentials`)
 * but ALSO at `/authorize`, as a bare 400 page it can't redirect back from.
 * So a registration this old is treated as already gone and re-registered
 * BEFORE the browser is ever sent to `/authorize` with it. One day short of
 * the server's TTL, so clock skew or a slow consent step can't straddle it. */
export const MCP_CLIENT_MAX_AGE_MS = 29 * 24 * 60 * 60 * 1000;

/** The prefix every key this provider stores starts with. */
export const MCP_OAUTH_STORAGE_PREFIX = STORAGE_PREFIX;

/** Every key this provider stores -- cleared on logout and account
 * deletion (`lib/browserAccountData.ts`). */
export const MCP_OAUTH_STORAGE_KEYS: readonly string[] = [
  CLIENT_INFO_KEY,
  TOKENS_KEY,
  CODE_VERIFIER_KEY,
  OAUTH_STATE_KEY,
  CLIENT_SAVED_AT_KEY,
];

/** The SDK's `OAuthClientProvider.invalidateCredentials` scope (MCP SDK
 * 1.30.0). */
export type McpCredentialScope = 'all' | 'client' | 'tokens' | 'verifier' | 'discovery';

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
  private readonly callbackUrl: string;
  private readonly now: () => number;

  constructor(callbackUrl: string, now: () => number = Date.now) {
    this.callbackUrl = callbackUrl;
    this.now = now;
  }

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

  /** The stored registration, or `undefined` when there is none or it is
   * old enough that the server has (or is about to have) expired it -- see
   * `MCP_CLIENT_MAX_AGE_MS`. A stale registration is discarded here, along
   * with the tokens issued to it (their refresh token is bound to that
   * dead `client_id`), so `auth()` falls through to a fresh Dynamic Client
   * Registration and a fresh authorization rather than sending the browser
   * to `/authorize` with a `client_id` the server will 400. */
  clientInformation(): OAuthClientInformationFull | undefined {
    const info = readJson<OAuthClientInformationFull>(CLIENT_INFO_KEY);
    if (info && this.isStale(info)) {
      this.invalidateCredentials('client');
      this.invalidateCredentials('tokens');
      return undefined;
    }
    return info;
  }

  saveClientInformation(clientInformation: OAuthClientInformationFull): void {
    localStorage.setItem(CLIENT_INFO_KEY, JSON.stringify(clientInformation));
    // Fallback age source for a registration response without
    // `client_id_issued_at` (RFC 7591 makes it optional).
    localStorage.setItem(CLIENT_SAVED_AT_KEY, String(this.now()));
  }

  private isStale(info: OAuthClientInformationFull): boolean {
    const now = this.now();
    // RFC 7591: seconds since the epoch; 0 means "never expires".
    if (typeof info.client_secret_expires_at === 'number' && info.client_secret_expires_at > 0) {
      if (info.client_secret_expires_at * 1000 <= now) return true;
    }
    let issuedAtMs: number | undefined;
    if (typeof info.client_id_issued_at === 'number' && info.client_id_issued_at > 0) {
      issuedAtMs = info.client_id_issued_at * 1000;
    } else {
      const savedAt = Number(localStorage.getItem(CLIENT_SAVED_AT_KEY));
      // No recorded time at all (saved by a provider version that predates
      // this check): treat as stale -- re-registering costs one request,
      // keeping a dead one strands the user on the server's 400 page.
      if (!Number.isFinite(savedAt) || savedAt <= 0) return true;
      issuedAtMs = savedAt;
    }
    return now - issuedAtMs >= MCP_CLIENT_MAX_AGE_MS;
  }

  /** The SDK's own recovery hook (`OAuthClientProvider.invalidateCredentials`,
   * MCP SDK 1.30.0): `auth()` calls it with `'all'` when the authorization
   * server answers `invalid_client`/`unauthorized_client` (an expired or
   * unknown registration) and with `'tokens'` on `invalid_grant` (a
   * revoked/expired refresh token or code), then retries once. Without it
   * the retry re-used the same dead `client_id`/refresh token and failed
   * identically, forever, until the visitor cleared site data.
   *
   * `'discovery'` is a no-op -- this provider caches no discovery state. */
  invalidateCredentials(scope: McpCredentialScope): void {
    if (scope === 'all' || scope === 'client') {
      localStorage.removeItem(CLIENT_INFO_KEY);
      localStorage.removeItem(CLIENT_SAVED_AT_KEY);
    }
    if (scope === 'all' || scope === 'tokens') {
      localStorage.removeItem(TOKENS_KEY);
    }
    if (scope === 'all' || scope === 'verifier') {
      localStorage.removeItem(CODE_VERIFIER_KEY);
    }
    if (scope === 'all') {
      localStorage.removeItem(OAUTH_STATE_KEY);
    }
  }

  /** Whether an authorization redirect was started from this browser and
   * never came back through `/chat/callback` (which always consumes the
   * stored `state`). The usual cause is the authorization server refusing
   * the stored `client_id` with a bare 400 page -- nothing to redirect back
   * from -- so the visitor returns to `/chat` by hand. */
  hasAbandonedAuthorization(): boolean {
    return localStorage.getItem(OAUTH_STATE_KEY) !== null;
  }

  tokens(): OAuthTokens | undefined {
    return readJson<OAuthTokens>(TOKENS_KEY);
  }

  saveTokens(tokens: OAuthTokens): void {
    localStorage.setItem(TOKENS_KEY, JSON.stringify(tokens));
  }

  /** `auth()` only gets here once it has no usable tokens (none stored, or
   * the refresh failed), so any tokens still stored are dead: dropping them
   * means a visitor who comes back without completing sign-in is offered
   * "Connect" instead of re-sending a dead token into the same 401 ->
   * redirect loop. */
  redirectToAuthorization(authorizationUrl: URL): void {
    localStorage.removeItem(TOKENS_KEY);
    window.location.href = authorizationUrl.toString();
  }

  saveCodeVerifier(codeVerifier: string): void {
    localStorage.setItem(CODE_VERIFIER_KEY, codeVerifier);
  }

  codeVerifier(): string {
    const verifier = localStorage.getItem(CODE_VERIFIER_KEY);
    if (!verifier) {
      throw new Error(
        'No PKCE code verifier found in localStorage -- the authorization flow was not started from this browser',
      );
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
