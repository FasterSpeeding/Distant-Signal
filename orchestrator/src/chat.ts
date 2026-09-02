/** The Anthropic Messages API tool-calling loop against distant-signal-mcp's
 * six tools (embedded-chatbot-option-b plan, Task 3 Step 4). */

import Anthropic from '@anthropic-ai/sdk';
import type { BetaRunnableTool } from '@anthropic-ai/sdk/lib/tools/BetaRunnableTool';
import { Client as McpClient } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

export type ChatEvent =
    | { type: 'text-delta'; text: string }
    | { type: 'tool-result'; toolName: string; structuredContent?: unknown }
    | { type: 'done' };

/** Exact system-prompt content, model choice, and max-turns bound are
 * implementation-time decisions -- the dual-mode design's own "Explicitly
 * out of scope" list names "a concrete... system-prompt design" as
 * un-designed by that document, and the plan does not resolve it either.
 * This is a starting point, flagged as unresearched (mirroring the
 * foundation plan's own posture toward its 90-day TTL/15-minute cache
 * TTL -- an honest starting figure, not a researched one), not a
 * carefully engineered prompt. */
const SYSTEM_PROMPT =
    'You are the Distant Signal assistant, helping a UK rail passenger check ' +
    'live departures, arrivals, service disruptions, and plan journeys. Use ' +
    'the available tools to answer with current, accurate information rather ' +
    'than guessing. Keep answers concise and focused on what the passenger ' +
    'asked.';

/** Same unresearched-starting-figure posture as the system prompt above. */
const MAX_ITERATIONS = 8;

/** Minimal shape distant-signal-mcp's `listTools()` result actually needs
 * here -- avoids importing the MCP SDK's own (much larger) tool type just
 * to read three fields off it. */
interface McpToolDefinition {
    name: string;
    description?: string;
    inputSchema: {
        type: 'object';
        properties?: Record<string, unknown> | null;
        required?: string[] | null;
        [key: string]: unknown;
    };
}

/** Builds one `BetaRunnableTool` per MCP tool, wired directly against
 * `mcpClient.callTool()` rather than through the Anthropic SDK's own
 * `mcpTool()` helper (`@anthropic-ai/sdk/helpers/beta/mcp`): that helper's
 * `run()` only ever returns the tool-result *content* the model needs back
 * (a deliberate simplification of its own -- there is no slot in
 * `BetaRunnableTool.run`'s return type for `structuredContent`), and this
 * loop needs `structuredContent` surfaced as its own `tool-result` SSE
 * event (Task 5's "track this leg" deep-link, `plan_journey`'s
 * `RenderedTrainLeg`). Reimplementing the thin wrapping directly here --
 * call the tool, hand `onToolResult` the structured half as a side
 * channel, hand the model back the text half -- avoids a second,
 * duplicate `callTool` round trip (which would double any side effects a
 * future tool might have) just to recover a value the helper already saw
 * and discarded.
 */
function buildRunnableTools(
    tools: McpToolDefinition[],
    mcpClient: McpClient,
    onToolResult: (event: { type: 'tool-result'; toolName: string; structuredContent?: unknown }) => void
): BetaRunnableTool[] {
    return tools.map((tool) => ({
        name: tool.name,
        description: tool.description ?? '',
        input_schema: tool.inputSchema as Anthropic.Beta.Messages.BetaTool['input_schema'],
        parse: (content: unknown) => content as Record<string, unknown>,
        run: async (args: Record<string, unknown>) => {
            const result = await mcpClient.callTool({ name: tool.name, arguments: args });
            if (result.structuredContent !== undefined) {
                onToolResult({ type: 'tool-result', toolName: tool.name, structuredContent: result.structuredContent });
            }
            const content = Array.isArray(result.content) ? result.content : [];
            const text = content
                .filter((block): block is { type: 'text'; text: string } => block.type === 'text')
                .map((block) => block.text)
                .join('\n');
            if (result.isError) {
                throw new Error(text || `${tool.name} failed`);
            }
            return text || '(no output)';
        }
    }));
}

export interface RunChatTurnOptions {
    anthropic: Anthropic;
    model: string;
    mcpUrl: string;
    mcpBearerToken: string;
    conversationHistory: Anthropic.Beta.Messages.BetaMessageParam[];
    userMessage: string;
}

/** Runs one user turn through the Anthropic Messages API tool-calling loop
 * against distant-signal-mcp's tools (`resolve_station`/`get_departures`/
 * `get_arrivals`/`get_service_detail`/`find_services`/`plan_journey`),
 * yielding streamed text deltas and any tool `structuredContent` as it
 * becomes available.
 *
 * A fresh MCP `Client`/`StreamableHTTPClientTransport` per turn, matching
 * distant-signal-mcp's own `/mcp` handler being stateless per request
 * (`app.ts`: "a fresh server and transport per request... any node can
 * serve any call") -- nothing here needs a connection to outlive one
 * user turn.
 */
export async function* runChatTurn(opts: RunChatTurnOptions): AsyncGenerator<ChatEvent> {
    const transport = new StreamableHTTPClientTransport(new URL(opts.mcpUrl), {
        requestInit: { headers: { Authorization: `Bearer ${opts.mcpBearerToken}` } }
    });
    const mcpClient = new McpClient({ name: 'distant-signal-orchestrator', version: '0.1.0' });
    await mcpClient.connect(transport);

    try {
        const { tools } = await mcpClient.listTools();

        // Buffered rather than yielded directly from inside the tool's own
        // `run()` above: a `BetaRunnableTool.run` isn't itself a generator
        // this function controls the scheduling of, so the only way to
        // surface its side effect through this async generator is via a
        // side channel drained between iterations below.
        const pendingToolResults: ChatEvent[] = [];
        const runnableTools = buildRunnableTools(tools as McpToolDefinition[], mcpClient, (event) => {
            pendingToolResults.push(event);
        });

        const runner = opts.anthropic.beta.messages.toolRunner({
            model: opts.model,
            max_tokens: 1024,
            system: SYSTEM_PROMPT,
            messages: [...opts.conversationHistory, { role: 'user', content: opts.userMessage }],
            tools: runnableTools,
            max_iterations: MAX_ITERATIONS,
            stream: true
        });

        for await (const messageStream of runner) {
            for await (const streamEvent of messageStream) {
                if (streamEvent.type === 'content_block_delta' && streamEvent.delta.type === 'text_delta') {
                    yield { type: 'text-delta', text: streamEvent.delta.text };
                }
            }
            // Tool calls the model made during this turn have now been
            // executed (the SDK's own toolRunner awaits `run()` before
            // moving to the next iteration) -- drain whatever
            // structuredContent they produced before requesting the next
            // turn.
            while (pendingToolResults.length > 0) {
                yield pendingToolResults.shift()!;
            }
        }

        yield { type: 'done' };
    } finally {
        await mcpClient.close();
    }
}
