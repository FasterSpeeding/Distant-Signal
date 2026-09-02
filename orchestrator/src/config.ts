/** Configuration for the chat orchestrator (embedded-chatbot-option-b
 * plan, Task 3) -- a DS-hosted service that holds DS's own Anthropic API
 * key and runs the Messages API tool-calling loop against
 * distant-signal-mcp's tools, ClusterIP-only (dual-mode design Decision
 * 2). Mirrors distant-signal-mcp's own src/config.ts conventions
 * (required()/positiveInteger() helpers, a set-but-empty variable counts
 * as unset) since this is a sibling internal TypeScript service, no
 * reason to diverge for a new one. */

export interface Config {
    port: number;
    /** DS's own paid Anthropic credential -- the entire reason this
     * service exists as its own process rather than folded into
     * distant-signal-mcp (dual-mode design Decision 2: that fork stays
     * public-facing without ever holding this). */
    anthropicApiKey: string;
    /** Model choice is an implementation-time decision the embedded-
     * chatbot-option-b plan's own Task 3 Step 4 note explicitly leaves
     * unresolved -- an unresearched starting figure, not a benchmarked
     * one, same posture as that plan's other unresearched constants (the
     * orchestrator-grant TTL, the DS line-catalogue cache TTL).
     * Configurable so a production deployment can change it without a
     * code change. */
    model: string;
    /** `crates/api`'s own base URL -- for the allowlist check
     * (GET /public/chatbot/access, Task 2) and nothing else; this
     * service never calls any other DS route (Global Constraints: it
     * only ever talks to distant-signal-mcp over MCP and to `api` for
     * this one narrow purpose). */
    dsApiBaseUrl: string;
    /** distant-signal-mcp's own in-cluster base URL -- both the
     * orchestrator-session token exchange (POST /token) and the actual
     * MCP tool calls (POST /mcp) go here. */
    railMcpBaseUrl: string;
    /** Shared secret authenticating this service's calls to
     * distant-signal-mcp's POST /token orchestrator-session grant
     * (Task 1) -- a SEPARATE credential from railMcp.internalCompleteToken
     * (frontend/'s consent bridge secret); see Task 1's own rationale. */
    orchestratorInternalToken: string;
}

function required(env: NodeJS.ProcessEnv, name: string): string {
    const value = env[name]?.trim();
    if (!value) {
        throw new Error(`Missing required environment variable: ${name}`);
    }
    return value;
}

/** A set-but-empty variable (`PORT=` in a .env file) counts as unset --
 * same rationale as distant-signal-mcp's own positiveInteger. */
function positiveInteger(env: NodeJS.ProcessEnv, name: string, fallback: number): number {
    const raw = env[name]?.trim();
    if (!raw) {
        return fallback;
    }
    const value = Number(raw);
    if (!Number.isInteger(value) || value <= 0) {
        throw new Error(`${name} must be a positive integer, got: ${raw}`);
    }
    return value;
}

export function loadConfig(env: NodeJS.ProcessEnv = process.env): Config {
    return {
        port: positiveInteger(env, 'PORT', 3001),
        anthropicApiKey: required(env, 'ANTHROPIC_API_KEY'),
        model: env.ANTHROPIC_MODEL?.trim() || 'claude-sonnet-4-5',
        dsApiBaseUrl: required(env, 'DS_API_BASE_URL').replace(/\/+$/, ''),
        railMcpBaseUrl: required(env, 'RAILMCP_BASE_URL').replace(/\/+$/, ''),
        orchestratorInternalToken: required(env, 'OAUTH_ORCHESTRATOR_INTERNAL_TOKEN')
    };
}
