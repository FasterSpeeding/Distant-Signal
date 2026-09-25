import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { StreamableHTTPError } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { renderWithMantine } from '@/test/render';
import { ChatPanel } from './ChatPanel';
import { setAnthropicApiKey } from '@/lib/anthropicKey';

const mockRunChatTurn = vi.fn();
vi.mock('@/lib/chatTurn', () => ({
  runChatTurn: (...args: unknown[]) => mockRunChatTurn(...args),
}));

function seedMcpTokens() {
  localStorage.setItem('ds-mcp-oauth:tokens', JSON.stringify({ access_token: 'tok', token_type: 'Bearer' }));
}

describe('ChatPanel', () => {
  beforeEach(() => {
    localStorage.clear();
    mockRunChatTurn.mockReset();
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
  });

  it('renders a placeholder prompt before any message is sent', () => {
    seedMcpTokens();
    setAnthropicApiKey('sk-ant-test');
    renderWithMantine(<ChatPanel />);
    expect(screen.getByText(/Ask about live departures/)).toBeInTheDocument();
  });

  it('shows a "no key" error when submitting without an Anthropic key set', async () => {
    seedMcpTokens();
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'when is the next train' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/set your anthropic api key/i)).toBeInTheDocument();
    expect(mockRunChatTurn).not.toHaveBeenCalled();
  });

  it('shows a "reconnect" error when no MCP token is stored', async () => {
    setAnthropicApiKey('sk-ant-test');
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'when is the next train' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/reconnect/i)).toBeInTheDocument();
    expect(mockRunChatTurn).not.toHaveBeenCalled();
  });

  // Review §3.1.6: the grape-theme spec reserves blue for `planned`
  // severity (lib/severity.ts's GROUP_COLOR), so the user bubble's
  // highlight moved off Mantine's default blue.
  it('gives the user\'s own message bubble a grape background, not blue', () => {
    seedMcpTokens();
    setAnthropicApiKey('sk-ant-test');
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'when is the next train' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    const bubble = screen.getByText('when is the next train').closest('.mantine-Card-root');
    expect(bubble).toHaveStyle({ background: 'var(--mantine-color-grape-0)' });
  });

  it('renders streamed text-delta events as the assistant reply', async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        yield { type: 'text-delta', text: 'Next ' };
        yield { type: 'text-delta', text: 'train is at 10:15.' };
        yield { type: 'done' };
      })(),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'when is the next train' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/next train is at 10:15/i)).toBeInTheDocument();
  });

  it('renders a "track this train" card for a plan_journey tool-result event', async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        yield {
          type: 'tool-result',
          toolName: 'plan_journey',
          structuredContent: {
            kind: 'train',
            from: { tiploc: 'KNGX', name: 'London Kings Cross', crs: 'KGX' },
            to: { tiploc: 'YORK', name: 'York', crs: 'YRK' },
            departure: '10:32',
            arrival: '12:01',
            departureAt: '2026-09-02T10:32:00Z',
            arrivalAt: '2026-09-02T12:01:00Z',
            operator: 'LNER',
            uid: 'A12345',
          },
        };
        yield { type: 'done' };
      })(),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'plan a trip' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByRole('link', { name: /track this train/i })).toBeInTheDocument();
  });

  it('shows the Anthropic-key error distinctly from a tool error on a 401', async () => {
    setAnthropicApiKey('sk-ant-bad');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        throw Object.assign(new Error('invalid api key'), { status: 401, constructor: { name: 'APIError' } });
      })(),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/anthropic api key was rejected/i)).toBeInTheDocument();
  });

  it('shows a distinct tool-error message for a non-auth failure', async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        throw new Error('get_departures failed: upstream Darwin timeout');
      })(),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/darwin timeout/i)).toBeInTheDocument();
  });

  // Bug: `classifyChatError` used to do a bare `/401|403|unauthoriz/i.test(message)`
  // substring match against the WHOLE error message -- any tool-error text
  // that merely contained "401" for an unrelated reason was misclassified
  // as a session-expiry error.
  it('does not misclassify a tool-error message that merely contains "401" as a session-expiry error', async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        throw new Error('Train reporting number 401 was cancelled');
      })(),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));

    expect(await screen.findByText(/train reporting number 401 was cancelled/i)).toBeInTheDocument();
    expect(screen.queryByText(/reconnect/i)).not.toBeInTheDocument();
  });

  // The real case this classifier exists for: `StreamableHTTPClientTransport`
  // throws a `StreamableHTTPError` carrying the actual HTTP status as
  // `.code` on a lapsed MCP session -- that structured status is checked
  // first now, rather than relying on a substring match at all.
  it('classifies a StreamableHTTPError with a 401 status as a session-expiry (reconnect) error', async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        throw new StreamableHTTPError(401, 'Server returned 401 after successful authentication');
      })(),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));

    expect(await screen.findByText(/reconnect/i)).toBeInTheDocument();
  });

  // A plain `Error` (no structured status at all -- e.g. a failed-tool-call
  // error built from raw upstream text) still gets the reconnect treatment
  // when it names an isolated "401"/"403"/"unauthorized" token, preserving
  // the pre-fix behaviour for the case that has no better signal available.
  it('still recognizes an isolated 401 token in a plain Error with no structured status', async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        throw new Error('get_departures failed: 401 Unauthorized');
      })(),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));

    expect(await screen.findByText(/reconnect/i)).toBeInTheDocument();
  });

  it('does not submit an empty or whitespace-only message', () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    renderWithMantine(<ChatPanel />);
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(mockRunChatTurn).not.toHaveBeenCalled();
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: '   ' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(mockRunChatTurn).not.toHaveBeenCalled();
  });
});
