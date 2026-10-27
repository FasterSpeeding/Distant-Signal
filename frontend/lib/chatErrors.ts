// The chat's error classification, kept out of ChatPanel's own module: it
// needs the Anthropic and MCP SDKs' error classes, and ChatPanel loads those
// SDKs on demand (`lib/chatRuntime.ts`) so they stay out of /chat's first
// load. Only `lib/chatRuntime.ts` imports this at runtime; ChatPanel takes
// the `ChatError` type alone.
import Anthropic from '@anthropic-ai/sdk';
import { StreamableHTTPError } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { UnauthorizedError } from '@modelcontextprotocol/sdk/client/auth.js';
import { OAuthError } from '@modelcontextprotocol/sdk/server/auth/errors.js';

export type ChatError =
  | { kind: 'no-key' }
  | { kind: 'anthropic-rejected' }
  | { kind: 'mcp-connect' }
  | { kind: 'mcp-reconnect' }
  | { kind: 'mcp-incomplete' }
  | { kind: 'sign-in-failed'; message: string }
  | { kind: 'tool-error'; message: string };

/** Anthropic's own `APIError` (and its `AuthenticationError` subclass, the
 * real shape a rejected/invalid key throws as) carries a numeric `status`.
 * Checked structurally as well as via `instanceof` so a test double or any
 * other Anthropic-error-shaped value (constructor named `APIError`, a
 * `status` of 401) is still recognized without depending on the real
 * class's prototype chain. */
function isAnthropicAuthError(err: unknown): boolean {
  if (err instanceof Anthropic.APIError) return err.status === 401;
  if (err && typeof err === 'object' && 'status' in err) {
    const status = (err as { status?: unknown }).status;
    const ctorName = (err as { constructor?: { name?: string } }).constructor?.name;
    return status === 401 && ctorName === 'APIError';
  }
  return false;
}

/** `client/streamableHttp.js`'s `StreamableHTTPError` carries the real HTTP
 * status as `.code` (set on 401/403/etc. responses from the MCP server --
 * see `streamableHttp.js`'s own `throw new StreamableHTTPError(response.status, ...)`
 * call sites). Checked structurally as well as via `instanceof`, mirroring
 * `isAnthropicAuthError` above, so a test double built with the same shape
 * is still recognized. */
function mcpHttpStatus(err: unknown): number | null {
  if (err instanceof StreamableHTTPError) return err.code ?? null;
  if (err && typeof err === 'object' && 'code' in err) {
    const code = (err as { code?: unknown }).code;
    const ctorName = (err as { constructor?: { name?: string } }).constructor?.name;
    if (typeof code === 'number' && ctorName === 'StreamableHTTPError') return code;
  }
  return null;
}

export function classifyChatError(err: unknown): ChatError {
  if (isAnthropicAuthError(err)) {
    return { kind: 'anthropic-rejected' };
  }
  // Bug: this used to be `/401|403|unauthoriz/i.test(message)` -- a plain
  // substring match against the whole error message, so any tool-error text
  // that merely *contained* "401" somewhere (a train reporting number, a
  // fare code, an unrelated upstream error string forwarded verbatim) was
  // misclassified as a session-expiry error, even though nothing about the
  // session had actually expired. A `StreamableHTTPError`'s real `.code`
  // status is authoritative and checked first; only when no structured
  // status is available (a bare `Error` thrown by `buildRunnableTools` for
  // a failed tool call, whose `message` is raw upstream/tool text) does this
  // fall back to a text match.
  //
  // The fallback deliberately does NOT match a bare "401"/"403" at all, even
  // an isolated one with word boundaries on both sides ("Train reporting
  // number 401 was cancelled" has exactly that shape) -- an isolated number
  // is not distinguishable from an HTTP status by word boundaries alone,
  // only by context. It matches "unauthoriz(ed/ation)"/"forbidden" instead,
  // the actual English words an upstream/tool error naming a real 401/403
  // reliably carries alongside the code (e.g. "401 Unauthorized", "403
  // Forbidden") -- present in a genuine auth failure's text, and exactly
  // what a merely-numeric coincidence like a train reporting number lacks.
  //
  // Bug (found via e2e/chat.spec.ts's "reconnect" case): a real 401/403 from
  // `/mcp` doesn't always surface as a `StreamableHTTPError` with a
  // structured `.code`. `client/streamableHttp.js`'s `send()` reacts to that
  // 401/403 by calling the SDK's own `auth()` (client/auth.js) to attempt
  // reauth *before* ever throwing -- and when that reauth attempt itself
  // fails (no cached discovery state, so the first step is an RFC 9728
  // `.well-known` fetch, or a client-registration `/register` call, either
  // of which can 401/403 too, e.g. because the well-known/registration
  // endpoints don't exist yet), `auth()` throws a bare `Error` whose message
  // is `discoverOAuthProtectedResourceMetadata`'s
  // "HTTP 401 trying to load well-known OAuth protected resource metadata."
  // or `parseErrorResponse`'s "HTTP 401: Invalid OAuth error response...".
  // Neither contains "unauthorized"/"forbidden", so the word-based match
  // above missed both, misclassifying a genuine session-expiry as a generic
  // tool-error. "HTTP 401"/"HTTP 403" (the literal word "HTTP" immediately
  // before the code) is a distinct, reliable signal that doesn't share the
  // false-positive risk a bare number does -- no upstream/tool text in this
  // app phrases anything else that way.
  // The SDK's own auth failures: `UnauthorizedError` (the transport's
  // reauth ended in a redirect, or found no usable credentials) and any
  // OAuth error response from the authorization server (e.g.
  // `invalid_client` for an expired registration surviving `auth()`'s
  // one retry). Both mean "sign in again", never a tool failure.
  if (isNamedError(err, UnauthorizedError, 'UnauthorizedError') || err instanceof OAuthError) {
    return { kind: 'mcp-reconnect' };
  }
  const status = mcpHttpStatus(err);
  if (status === 401 || status === 403) {
    return { kind: 'mcp-reconnect' };
  }
  const message = err instanceof Error ? err.message : '';
  if (status === null && /\b(unauthoriz(?:ed|ation)?|forbidden)\b|\bHTTP\s+(?:401|403)\b/i.test(message)) {
    return { kind: 'mcp-reconnect' };
  }
  return { kind: 'tool-error', message };
}

function isNamedError(err: unknown, ctor: abstract new (...args: never[]) => unknown, name: string): boolean {
  if (err instanceof ctor) return true;
  return !!err && typeof err === 'object' && (err as { constructor?: { name?: string } }).constructor?.name === name;
}
