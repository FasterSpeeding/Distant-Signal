/** The Anthropic Messages API tool-calling loop against distant-signal-mcp's
 * six tools -- relocated verbatim in shape from orchestrator/src/chat.ts
 * (now deleted, see this plan's Task 5) into the browser, per the
 * client-side-tokens design doc's Decision 1. The loop logic itself is
 * NOT redesigned here, only where it runs and how its two clients
 * authenticate. */
import Anthropic from '@anthropic-ai/sdk';
import type { BetaRunnableTool } from '@anthropic-ai/sdk/lib/tools/BetaRunnableTool';
import { Client as McpClient } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import type { OAuthClientProvider } from '@modelcontextprotocol/sdk/client/auth.js';

export type ChatEvent =
  | { type: 'text-delta'; text: string }
  | { type: 'tool-result'; toolName: string; structuredContent?: unknown }
  | { type: 'done' };

// Same unresearched-starting-figure posture orchestrator/src/chat.ts's own
// SYSTEM_PROMPT/MAX_ITERATIONS carried -- the design doc's own "Explicitly
// out of scope" list still leaves this un-designed.
const SYSTEM_PROMPT =
  'You are the Distant Signal assistant, helping a UK rail passenger check ' +
  'live departures, arrivals, service disruptions, and plan journeys. Use ' +
  'the available tools to answer with current, accurate information rather ' +
  'than guessing. Keep answers concise and focused on what the passenger ' +
  'asked.';
const MAX_ITERATIONS = 8;

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

function buildRunnableTools(
  tools: McpToolDefinition[],
  mcpClient: McpClient,
  onToolResult: (event: { type: 'tool-result'; toolName: string; structuredContent?: unknown }) => void,
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
    },
  }));
}

export interface RunChatTurnOptions {
  /** Constructed by the caller with `dangerouslyAllowBrowser: true` and
   * the user's own key (frontend/lib/anthropicKey.ts) -- this module never
   * reads or constructs the key itself. */
  anthropic: Anthropic;
  model: string;
  mcpUrl: string;
  /** Drives StreamableHTTPClientTransport's own automatic reauth-on-401
   * (client/streamableHttp.js calling client/auth.js's `auth()`
   * internally) -- see BrowserMcpOAuthProvider (frontend/lib/mcpOAuthProvider.ts). */
  mcpAuthProvider: OAuthClientProvider;
  conversationHistory: Anthropic.Beta.Messages.BetaMessageParam[];
  userMessage: string;
}

export async function* runChatTurn(opts: RunChatTurnOptions): AsyncGenerator<ChatEvent> {
  const transport = new StreamableHTTPClientTransport(new URL(opts.mcpUrl), {
    authProvider: opts.mcpAuthProvider,
  });
  const mcpClient = new McpClient({ name: 'distant-signal-chat', version: '0.1.0' });
  await mcpClient.connect(transport);

  try {
    const { tools } = await mcpClient.listTools();

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
      stream: true,
    });

    for await (const messageStream of runner) {
      for await (const streamEvent of messageStream) {
        if (streamEvent.type === 'content_block_delta' && streamEvent.delta.type === 'text_delta') {
          yield { type: 'text-delta', text: streamEvent.delta.text };
        }
      }
      while (pendingToolResults.length > 0) {
        yield pendingToolResults.shift()!;
      }
    }

    yield { type: 'done' };
  } finally {
    await mcpClient.close();
  }
}
