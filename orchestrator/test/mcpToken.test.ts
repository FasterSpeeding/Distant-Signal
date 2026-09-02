import { beforeEach, describe, expect, it, vi } from 'vitest';
import { clearMcpTokenCache, getMcpToken } from '../src/mcpToken.js';

function stubFetch(expiresIn: number): { fetchImpl: typeof fetch; calls: () => number } {
    let calls = 0;
    const fetchImpl = vi.fn(async () => {
        calls += 1;
        return new Response(JSON.stringify({ access_token: `token-${calls}`, expires_in: expiresIn }), { status: 200 });
    }) as unknown as typeof fetch;
    return { fetchImpl, calls: () => calls };
}

describe('getMcpToken', () => {
    beforeEach(() => {
        clearMcpTokenCache();
    });

    it('exchanges the session for a token via the orchestrator-session grant', async () => {
        const { fetchImpl } = stubFetch(3600);
        const token = await getMcpToken('https://mcp.example.com', 'internal-token', 'raw-session', fetchImpl);
        expect(token).toBe('token-1');
        const [url, init] = (fetchImpl as unknown as { mock: { calls: [string, RequestInit][] } }).mock.calls[0]!;
        expect(url).toBe('https://mcp.example.com/token');
        expect((init.headers as Record<string, string>)['X-Orchestrator-Internal-Token']).toBe('internal-token');
        const body = new URLSearchParams(init.body as string);
        expect(body.get('grant_type')).toBe('urn:distant-signal:orchestrator-session');
        expect(body.get('ds_session_cookie_value')).toBe('raw-session');
    });

    it('does not re-hit /token for a second call within the cache window', async () => {
        const { fetchImpl, calls } = stubFetch(3600);
        await getMcpToken('https://mcp.example.com', 'internal-token', 'raw-session', fetchImpl);
        await getMcpToken('https://mcp.example.com', 'internal-token', 'raw-session', fetchImpl);
        expect(calls()).toBe(1);
    });

    it('re-exchanges once the cached token has expired', async () => {
        vi.useFakeTimers();
        try {
            const { fetchImpl, calls } = stubFetch(60); // 60s TTL, well under the 30s headroom window
            await getMcpToken('https://mcp.example.com', 'internal-token', 'raw-session', fetchImpl);
            vi.advanceTimersByTime(61_000);
            await getMcpToken('https://mcp.example.com', 'internal-token', 'raw-session', fetchImpl);
            expect(calls()).toBe(2);
        } finally {
            vi.useRealTimers();
        }
    });

    it('caches per-session, not globally -- a different session gets its own exchange', async () => {
        const { fetchImpl, calls } = stubFetch(3600);
        await getMcpToken('https://mcp.example.com', 'internal-token', 'session-a', fetchImpl);
        await getMcpToken('https://mcp.example.com', 'internal-token', 'session-b', fetchImpl);
        expect(calls()).toBe(2);
    });

    it('throws when the exchange fails', async () => {
        const failingFetch = vi.fn(async () => new Response('nope', { status: 401 })) as unknown as typeof fetch;
        await expect(getMcpToken('https://mcp.example.com', 'internal-token', 'raw-session', failingFetch)).rejects.toThrow(
            /token exchange failed/
        );
    });
});
