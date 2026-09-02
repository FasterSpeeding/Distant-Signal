import { describe, it, expect, vi } from 'vitest';
import Anthropic from '@anthropic-ai/sdk';
import { runChatTurn } from './chatTurn';

// Mocks the Anthropic SDK and MCP Client rather than hitting real network
// -- this loop's own control flow (drain text deltas, drain tool results
// between iterations, yield `done`) is what's under test here, not the
// real API integration (that's Task 11's Playwright coverage's job).
vi.mock('@modelcontextprotocol/sdk/client/index.js', () => ({
  Client: vi.fn().mockImplementation(() => ({
    connect: vi.fn(),
    listTools: vi.fn().mockResolvedValue({
      tools: [{ name: 'resolve_station', description: 'resolve a station', inputSchema: { type: 'object' } }],
    }),
    callTool: vi.fn().mockResolvedValue({ content: [{ type: 'text', text: 'York' }], structuredContent: { kind: 'station' } }),
    close: vi.fn(),
  })),
}));
vi.mock('@modelcontextprotocol/sdk/client/streamableHttp.js', () => ({
  StreamableHTTPClientTransport: vi.fn(),
}));

function fakeAnthropic(streamEvents: unknown[]): Anthropic {
  return {
    beta: {
      messages: {
        toolRunner: vi.fn().mockReturnValue(
          (async function* () {
            yield (async function* () {
              for (const event of streamEvents) yield event;
            })();
          })(),
        ),
      },
    },
  } as unknown as Anthropic;
}

describe('runChatTurn', () => {
  it('yields text-delta events for each text_delta stream event', async () => {
    const anthropic = fakeAnthropic([
      { type: 'content_block_delta', delta: { type: 'text_delta', text: 'Hello' } },
      { type: 'content_block_delta', delta: { type: 'text_delta', text: ' there' } },
    ]);
    const events = [];
    for await (const event of runChatTurn({
      anthropic,
      model: 'claude-x',
      mcpUrl: 'https://mcp.example.com/mcp',
      mcpAuthProvider: {} as never,
      conversationHistory: [],
      userMessage: 'hi',
    })) {
      events.push(event);
    }
    expect(events).toContainEqual({ type: 'text-delta', text: 'Hello' });
    expect(events).toContainEqual({ type: 'text-delta', text: ' there' });
    expect(events[events.length - 1]).toEqual({ type: 'done' });
  });

  it('ignores non-text_delta stream events', async () => {
    const anthropic = fakeAnthropic([{ type: 'content_block_delta', delta: { type: 'input_json_delta', partial_json: '{}' } }]);
    const events = [];
    for await (const event of runChatTurn({
      anthropic,
      model: 'claude-x',
      mcpUrl: 'https://mcp.example.com/mcp',
      mcpAuthProvider: {} as never,
      conversationHistory: [],
      userMessage: 'hi',
    })) {
      events.push(event);
    }
    expect(events).toEqual([{ type: 'done' }]);
  });
});
