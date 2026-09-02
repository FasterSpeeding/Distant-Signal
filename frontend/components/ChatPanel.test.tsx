import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
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
