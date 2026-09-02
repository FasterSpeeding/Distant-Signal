/** Exchanges a forwarded DS session for a distant-signal-mcp bearer token
 * via Task 1's `urn:distant-signal:orchestrator-session` grant, with a
 * per-session in-memory cache (embedded-chatbot-option-b plan, Task 3
 * Step 3). */

import { createHash } from 'node:crypto';

interface CachedToken {
    accessToken: string;
    expiresAt: number;
}

/** Keyed by a hash of the raw session cookie value, never the raw value
 * itself -- avoids holding a second, in-memory-plaintext copy of a live DS
 * session credential alongside the one already flowing through each
 * request.
 *
 * An in-process `Map`, not Redis: unlike the foundation plan's own stores
 * (which must survive a pod restart because an *external, long-lived*
 * Claude.ai connection depends on them), this cache only ever saves one
 * redundant `/token` round trip within a single conversation's lifetime;
 * if this process restarts mid-conversation, the next message simply
 * re-exchanges, at the cost of one extra request, not a broken feature. */
const cache = new Map<string, CachedToken>();

function sha256Hex(input: string): string {
    return createHash('sha256').update(input).digest('hex');
}

/** Exported for tests only -- lets a test suite start from a known-empty
 * cache rather than depending on module-load/test-execution order. */
export function clearMcpTokenCache(): void {
    cache.clear();
}

/** 30s of headroom before the token's own expiry, so a request that lands
 * right at the boundary doesn't get handed a token that expires mid-flight
 * against distant-signal-mcp. */
const EXPIRY_HEADROOM_MS = 30_000;

export async function getMcpToken(
    railMcpBaseUrl: string,
    orchestratorInternalToken: string,
    dsSessionCookieValue: string,
    fetchImpl: typeof fetch = fetch
): Promise<string> {
    const key = sha256Hex(dsSessionCookieValue);
    const cached = cache.get(key);
    if (cached && cached.expiresAt > Date.now() + EXPIRY_HEADROOM_MS) {
        return cached.accessToken;
    }

    const res = await fetchImpl(`${railMcpBaseUrl}/token`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/x-www-form-urlencoded',
            'X-Orchestrator-Internal-Token': orchestratorInternalToken
        },
        body: new URLSearchParams({
            grant_type: 'urn:distant-signal:orchestrator-session',
            ds_session_cookie_value: dsSessionCookieValue
        })
    });
    if (!res.ok) {
        throw new Error(`token exchange failed: ${res.status}`);
    }
    const body = (await res.json()) as { access_token: string; expires_in: number };
    cache.set(key, { accessToken: body.access_token, expiresAt: Date.now() + body.expires_in * 1000 });
    return body.access_token;
}
