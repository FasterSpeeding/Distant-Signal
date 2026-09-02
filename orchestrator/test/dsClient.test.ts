import { describe, expect, it, vi } from 'vitest';
import { checkChatbotAccess } from '../src/dsClient.js';

function stubFetch(status: number): typeof fetch {
    return vi.fn(async () => new Response('{}', { status })) as unknown as typeof fetch;
}

describe('checkChatbotAccess', () => {
    it('returns allowed for a 200', async () => {
        const result = await checkChatbotAccess('https://ds.example.com', 'distant_signal_session=abc', stubFetch(200));
        expect(result).toBe('allowed');
    });

    it('returns unauthenticated for a 401', async () => {
        const result = await checkChatbotAccess('https://ds.example.com', '', stubFetch(401));
        expect(result).toBe('unauthenticated');
    });

    it('returns forbidden for a 403', async () => {
        const result = await checkChatbotAccess('https://ds.example.com', 'distant_signal_session=abc', stubFetch(403));
        expect(result).toBe('forbidden');
    });

    it('fails closed (forbidden) for an unrecognised status, never an ambiguous allow', async () => {
        const result = await checkChatbotAccess('https://ds.example.com', 'distant_signal_session=abc', stubFetch(500));
        expect(result).toBe('forbidden');
    });

    it('fails closed (forbidden) when the fetch itself throws', async () => {
        const throwingFetch = vi.fn(async () => {
            throw new Error('network down');
        }) as unknown as typeof fetch;
        const result = await checkChatbotAccess('https://ds.example.com', 'distant_signal_session=abc', throwingFetch);
        expect(result).toBe('forbidden');
    });

    it('forwards the Cookie header verbatim', async () => {
        const fetchImpl = vi.fn(async () => new Response('{}', { status: 200 })) as unknown as typeof fetch;
        await checkChatbotAccess('https://ds.example.com', 'distant_signal_session=abc; theme=dark', fetchImpl);
        const [, init] = (fetchImpl as unknown as { mock: { calls: [string, RequestInit][] } }).mock.calls[0]!;
        expect((init.headers as Record<string, string>).Cookie).toBe('distant_signal_session=abc; theme=dark');
    });

    it('calls GET /public/chatbot/access on the given base URL', async () => {
        const fetchImpl = vi.fn(async () => new Response('{}', { status: 200 })) as unknown as typeof fetch;
        await checkChatbotAccess('https://ds.example.com', '', fetchImpl);
        const [url] = (fetchImpl as unknown as { mock: { calls: [string, RequestInit][] } }).mock.calls[0]!;
        expect(url).toBe('https://ds.example.com/public/chatbot/access');
    });
});
