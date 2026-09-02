/** `POST /chat` -- the orchestrator's only route (embedded-chatbot-option-b
 * plan, Task 3 Step 5). `ClusterIP`-only; `frontend/app/api/chat/route.ts`
 * (Task 4) is this service's only caller. */

import express from 'express';
import type { Express } from 'express';
import type Anthropic from '@anthropic-ai/sdk';
import { checkChatbotAccess, type ChatbotAccess } from './dsClient.js';
import { getMcpToken } from './mcpToken.js';
import { runChatTurn, type ChatEvent } from './chat.js';
import type { Config } from './config.js';

const SESSION_COOKIE_NAME = 'distant_signal_session';

/** Parses out `distant_signal_session=...` specifically, mirroring
 * `frontend/app/connect-claude/authorize/route.ts`'s own
 * `SESSION_COOKIE_NAME` constant (foundation plan Task 6) -- kept as a
 * literal string here too, not imported, since this is a different
 * repository/deploy unit with no shared package to import it from. */
function extractSessionCookieValue(cookieHeader: string): string | undefined {
    for (const part of cookieHeader.split(';')) {
        const eq = part.indexOf('=');
        if (eq === -1) {
            continue;
        }
        const name = part.slice(0, eq).trim();
        if (name === SESSION_COOKIE_NAME) {
            return part.slice(eq + 1).trim();
        }
    }
    return undefined;
}

export interface AppDeps {
    config: Config;
    anthropic: Anthropic;
    /** Injectable seams for testing (Task 3 Step 6's own smoke-test
     * framing: confirm the allowlist gate runs BEFORE either of these
     * ever gets called) and so a real deployment's fetch/Anthropic
     * behaviour is exercised through the same production code path a
     * test double substitutes here. */
    checkAccess?: typeof checkChatbotAccess;
    getToken?: typeof getMcpToken;
    runTurn?: typeof runChatTurn;
}

export function buildApp(deps: AppDeps): Express {
    const { config, anthropic } = deps;
    const checkAccess = deps.checkAccess ?? checkChatbotAccess;
    const getToken = deps.getToken ?? getMcpToken;
    const runTurn = deps.runTurn ?? runChatTurn;

    const app = express();
    app.use(express.json());

    app.get('/healthz', (_req, res) => {
        res.json({ status: 'ok' });
    });

    app.post('/chat', async (req, res) => {
        const cookieHeader = req.header('Cookie') ?? '';

        // Step 1: the allowlist gate -- MUST run, and MUST reject, before
        // either token exchange or the Anthropic call below. This is the
        // entire cost-protecting property Task 2/3 exist for.
        let access: ChatbotAccess;
        try {
            access = await checkAccess(config.dsApiBaseUrl, cookieHeader);
        } catch (err) {
            console.error('allowlist check failed:', err);
            res.status(403).json({ error: 'chatbot_not_available' });
            return;
        }
        if (access === 'unauthenticated') {
            res.status(401).json({ error: 'unauthenticated' });
            return;
        }
        if (access === 'forbidden') {
            res.status(403).json({ error: 'chatbot_not_available' });
            return;
        }

        const sessionCookieValue = extractSessionCookieValue(cookieHeader);
        if (!sessionCookieValue) {
            // Allowed by the allowlist check (which read the same Cookie
            // header) but no distant_signal_session cookie was actually
            // present -- should not happen in practice (the allowlist
            // check itself requires a session), defensive rather than
            // reachable in normal operation.
            res.status(401).json({ error: 'unauthenticated' });
            return;
        }

        const body = req.body as { conversationId?: string; message?: string };
        if (!body.message || typeof body.message !== 'string') {
            res.status(400).json({ error: 'invalid_request' });
            return;
        }

        let mcpToken: string;
        try {
            mcpToken = await getToken(config.railMcpBaseUrl, config.orchestratorInternalToken, sessionCookieValue);
        } catch (err) {
            console.error('mcp token exchange failed:', err);
            res.status(502).json({ error: 'upstream_unavailable' });
            return;
        }

        res.setHeader('Content-Type', 'text/event-stream');
        res.setHeader('Cache-Control', 'no-cache');
        res.setHeader('Connection', 'keep-alive');
        res.flushHeaders();

        try {
            const events: AsyncGenerator<ChatEvent> = runTurn({
                anthropic,
                model: config.model,
                mcpUrl: `${config.railMcpBaseUrl}/mcp`,
                mcpBearerToken: mcpToken,
                // Per-conversation history is not persisted anywhere yet
                // (no store for it exists in this task) -- every request
                // is a fresh, single-turn conversation. `conversationId`
                // is accepted in the request shape for forward
                // compatibility (Task 5's frontend can start sending it
                // once this exists) but unused today.
                conversationHistory: [],
                userMessage: body.message
            });
            for await (const event of events) {
                res.write(`data: ${JSON.stringify(event)}\n\n`);
            }
        } catch (err) {
            console.error('chat turn failed:', err);
            res.write(`data: ${JSON.stringify({ type: 'error', error: 'chat_failed' })}\n\n`);
        } finally {
            res.end();
        }
    });

    return app;
}
