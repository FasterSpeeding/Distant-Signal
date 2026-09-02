import type Anthropic from '@anthropic-ai/sdk';
import request from 'supertest';
import { describe, expect, it, vi } from 'vitest';
import { buildApp } from '../src/app.js';
import type { Config } from '../src/config.js';
import type { ChatEvent } from '../src/chat.js';

const config: Config = {
    port: 3001,
    anthropicApiKey: 'test-anthropic-key',
    model: 'claude-sonnet-4-5',
    dsApiBaseUrl: 'https://ds.example.com',
    railMcpBaseUrl: 'https://mcp.example.com',
    orchestratorInternalToken: 'orchestrator-internal-token-for-tests'
};

// This suite exercises the app's own wiring (the allowlist-gate-before-spend
// property Task 2/3 exist for, and the mcp token/chat-turn hand-off), not
// the real Anthropic/MCP calls those functions make -- see chat.ts's own
// module for the actual tool-calling loop, and mcpToken.test.ts/
// dsClient.test.ts for those functions' own unit coverage.

async function* fakeTurn(): AsyncGenerator<ChatEvent> {
    yield { type: 'text-delta', text: 'hello' };
    yield { type: 'done' };
}

describe('POST /chat', () => {
    it('401s and never reaches token exchange or the chat turn when unauthenticated', async () => {
        const checkAccess = vi.fn(async () => 'unauthenticated' as const);
        const getToken = vi.fn();
        const runTurn = vi.fn();
        const app = buildApp({ config, anthropic: {} as Anthropic, checkAccess, getToken, runTurn });

        const res = await request(app).post('/chat').send({ message: 'when is the next train?' });
        expect(res.status).toBe(401);
        expect(getToken).not.toHaveBeenCalled();
        expect(runTurn).not.toHaveBeenCalled();
    });

    it('403s and never reaches token exchange or the chat turn -- the cost-protecting property -- for a non-allowlisted user', async () => {
        const checkAccess = vi.fn(async () => 'forbidden' as const);
        const getToken = vi.fn();
        const runTurn = vi.fn();
        const app = buildApp({ config, anthropic: {} as Anthropic, checkAccess, getToken, runTurn });

        const res = await request(app)
            .post('/chat')
            .set('Cookie', 'distant_signal_session=raw-session')
            .send({ message: 'when is the next train?' });
        expect(res.status).toBe(403);
        expect(res.body.error).toBe('chatbot_not_available');
        expect(getToken).not.toHaveBeenCalled();
        expect(runTurn).not.toHaveBeenCalled();
    });

    it('exchanges a token and streams the chat turn for an allowed session', async () => {
        const checkAccess = vi.fn(async () => 'allowed' as const);
        const getToken = vi.fn(async () => 'mcp-bearer-token');
        const runTurn = vi.fn(fakeTurn);
        const app = buildApp({ config, anthropic: {} as Anthropic, checkAccess, getToken, runTurn });

        const res = await request(app)
            .post('/chat')
            .set('Cookie', 'distant_signal_session=raw-session')
            .send({ message: 'when is the next train?' });
        expect(res.status).toBe(200);
        expect(res.headers['content-type']).toContain('text/event-stream');
        expect(getToken).toHaveBeenCalledWith(config.railMcpBaseUrl, config.orchestratorInternalToken, 'raw-session');
        expect(runTurn).toHaveBeenCalledOnce();
        expect(res.text).toContain('"type":"text-delta"');
        expect(res.text).toContain('"type":"done"');
    });

    it('forwards the Cookie header\'s session value, not the whole header, to the token exchange', async () => {
        const checkAccess = vi.fn(async () => 'allowed' as const);
        const getToken = vi.fn(async () => 'mcp-bearer-token');
        const runTurn = vi.fn(fakeTurn);
        const app = buildApp({ config, anthropic: {} as Anthropic, checkAccess, getToken, runTurn });

        await request(app)
            .post('/chat')
            .set('Cookie', 'theme=dark; distant_signal_session=raw-session; other=x')
            .send({ message: 'hi' });
        expect(getToken).toHaveBeenCalledWith(config.railMcpBaseUrl, config.orchestratorInternalToken, 'raw-session');
    });

    it('400s a request with no message', async () => {
        const checkAccess = vi.fn(async () => 'allowed' as const);
        const app = buildApp({
            config,
            anthropic: {} as Anthropic,
            checkAccess,
            getToken: vi.fn(),
            runTurn: vi.fn()
        });
        const res = await request(app)
            .post('/chat')
            .set('Cookie', 'distant_signal_session=raw-session')
            .send({});
        expect(res.status).toBe(400);
    });

    it('502s when the mcp token exchange fails', async () => {
        const checkAccess = vi.fn(async () => 'allowed' as const);
        const getToken = vi.fn(async () => {
            throw new Error('token exchange failed: 401');
        });
        const runTurn = vi.fn();
        const app = buildApp({ config, anthropic: {} as Anthropic, checkAccess, getToken, runTurn });

        const res = await request(app)
            .post('/chat')
            .set('Cookie', 'distant_signal_session=raw-session')
            .send({ message: 'hi' });
        expect(res.status).toBe(502);
        expect(runTurn).not.toHaveBeenCalled();
    });
});
