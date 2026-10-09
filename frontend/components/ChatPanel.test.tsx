import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { StreamableHTTPError } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { renderWithMantine } from '@/test/render';
import { ChatPanel } from './ChatPanel';
import { CHAT_AI_NOTE } from './AiGeneratedBadge';
import { setAnthropicApiKey } from '@/lib/anthropicKey';

const mockRunChatTurn = vi.fn();
vi.mock('@/lib/chatTurn', () => ({
  runChatTurn: (...args: unknown[]) => mockRunChatTurn(...args),
}));

const mockStartMcpSignIn = vi.fn();
vi.mock('@/lib/mcpAuthorization', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/mcpAuthorization')>()),
  startMcpSignIn: (...args: unknown[]) => mockStartMcpSignIn(...args),
}));

function seedMcpTokens() {
  localStorage.setItem('ds-mcp-oauth:tokens', JSON.stringify({ access_token: 'tok', token_type: 'Bearer' }));
}

describe('ChatPanel', () => {
  beforeEach(() => {
    localStorage.clear();
    mockRunChatTurn.mockReset();
    mockStartMcpSignIn.mockReset();
  });

  it('renders a placeholder prompt before any message is sent', () => {
    seedMcpTokens();
    setAnthropicApiKey('sk-ant-test');
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    expect(screen.getByText(/Ask about live departures/)).toBeInTheDocument();
  });

  it('shows a "no key" error when submitting without an Anthropic key set', async () => {
    seedMcpTokens();
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'when is the next train' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/set your anthropic api key/i)).toBeInTheDocument();
    expect(mockRunChatTurn).not.toHaveBeenCalled();
  });

  it('offers "Connect" when no MCP token has ever been stored', async () => {
    setAnthropicApiKey('sk-ant-test');
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'when is the next train' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/connect to the rail data service/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Connect' })).toBeInTheDocument();
    expect(mockRunChatTurn).not.toHaveBeenCalled();
  });

  // Review §3.1.6: the grape-theme spec reserves blue for `planned`
  // severity (lib/severity.ts's GROUP_COLOR), so the user bubble's
  // highlight moved off Mantine's default blue.
  it("gives the user's own message bubble a grape background, not blue", () => {
    seedMcpTokens();
    setAnthropicApiKey('sk-ant-test');
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
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
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
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
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
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
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
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
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText(/couldn.t answer that\. try again/i)).toBeInTheDocument();
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
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));

    expect(await screen.findByText(/couldn.t answer that\. try again/i)).toBeInTheDocument();
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
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));

    expect(await screen.findByRole('button', { name: 'Reconnect' })).toBeInTheDocument();
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
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));

    expect(await screen.findByRole('button', { name: 'Reconnect' })).toBeInTheDocument();
  });

  // Bug found by e2e/chat.spec.ts's "reconnect" case: `@modelcontextprotocol/sdk`'s
  // own `auth()` (client/auth.js) throws a bare `Error` -- not a
  // `StreamableHTTPError` -- when its OWN reauth attempt (triggered by the
  // real 401/403 from `/mcp`) fails, e.g. `discoverOAuthProtectedResourceMetadata`'s
  // "HTTP 401 trying to load well-known OAuth protected resource metadata."
  // when the RFC 9728 `.well-known` endpoint itself 401s. That message has
  // no structured `.code` and contains neither "unauthorized" nor
  // "forbidden", so it fell through to `tool-error` instead of
  // `mcp-reconnect` even though it is a genuine session-expiry.
  it('recognizes an MCP OAuth-discovery failure ("HTTP 401 trying to...") as a session-expiry error', async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        throw new Error('HTTP 401 trying to load well-known OAuth protected resource metadata.');
      })(),
    );
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));

    expect(await screen.findByRole('button', { name: 'Reconnect' })).toBeInTheDocument();
  });

  it('does not submit an empty or whitespace-only message', () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(mockRunChatTurn).not.toHaveBeenCalled();
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: '   ' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(mockRunChatTurn).not.toHaveBeenCalled();
  });

  // LEG-16: the chat's output is AI-generated and can be wrong.
  it('always shows a visible "AI-generated, may be inaccurate" note', () => {
    seedMcpTokens();
    setAnthropicApiKey('sk-ant-test');
    const { container } = renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    expect(container.querySelector('[data-ai-note]')).toHaveTextContent(CHAT_AI_NOTE);
    expect(CHAT_AI_NOTE).toMatch(/may be inaccurate/);
  });

  it("labels each assistant reply, and not the user's own messages, as AI-generated", async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        yield { type: 'text-delta', text: 'Next train is at 10:15.' };
        yield { type: 'done' };
      })(),
    );
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'when is the next train' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    await screen.findByText(/next train is at 10:15/i);
    const badges = screen.getAllByText('AI summary');
    expect(badges).toHaveLength(1);
    expect(badges[0]!.closest('[data-ai-badge]')).toHaveAccessibleDescription(CHAT_AI_NOTE);
    const userBubble = screen.getByText('when is the next train').closest('.mantine-Card-root')!;
    expect(userBubble.querySelector('[data-ai-badge]')).toBeNull();
  });

  // FE-2: the MCP URL comes from the server-supplied prop, not a
  // build-time NEXT_PUBLIC_ inline.
  it('passes `${mcpServerUrl}/mcp` to runChatTurn, ignoring a trailing slash', async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn.mockReturnValue(
      (async function* () {
        yield { type: 'done' };
      })(),
    );
    renderWithMantine(<ChatPanel mcpServerUrl="https://runtime-mcp.example.com/" />);
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    await vi.waitFor(() => expect(mockRunChatTurn).toHaveBeenCalled());
    expect(mockRunChatTurn.mock.calls[0]![0]).toMatchObject({ mcpUrl: 'https://runtime-mcp.example.com/mcp' });
  });

  // FE-11: each turn's streamed text lands on its own assistant message.
  it("routes a second turn's streamed text to the second assistant message, leaving the first intact", async () => {
    setAnthropicApiKey('sk-ant-test');
    seedMcpTokens();
    mockRunChatTurn
      .mockReturnValueOnce(
        (async function* () {
          yield { type: 'text-delta', text: 'First answer.' };
          yield { type: 'done' };
        })(),
      )
      .mockReturnValueOnce(
        (async function* () {
          await Promise.resolve();
          yield { type: 'text-delta', text: 'Second ' };
          yield { type: 'text-delta', text: 'answer.' };
          yield { type: 'done' };
        })(),
      );
    renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
    const input = screen.getByPlaceholderText(/ask about/i);
    fireEvent.change(input, { target: { value: 'one' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText('First answer.')).toBeInTheDocument();
    await vi.waitFor(() => expect(screen.getByPlaceholderText(/ask about/i)).not.toBeDisabled());
    fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'two' } });
    fireEvent.click(screen.getByRole('button', { name: /send/i }));
    expect(await screen.findByText('Second answer.')).toBeInTheDocument();
    expect(screen.getByText('First answer.')).toBeInTheDocument();
    const texts = screen.getAllByText(/answer\.|^one$|^two$/).map((el) => el.textContent);
    expect(texts).toEqual(['one', 'First answer.', 'two', 'Second answer.']);
  });

  // DQ12 (FE-6): tools not known to be read-only wait for the passenger.
  describe('tool confirmation', () => {
    function confirmingTurn() {
      mockRunChatTurn.mockImplementation(
        (opts: { confirmToolCall: (r: { toolName: string; args: Record<string, unknown> }) => Promise<boolean> }) =>
          (async function* () {
            const allowed = await opts.confirmToolCall({ toolName: 'track_train', args: { uid: 'C12345' } });
            yield { type: 'text-delta', text: allowed ? 'Tool was allowed.' : 'Tool was declined.' };
            yield { type: 'done' };
          })(),
      );
    }

    async function send() {
      renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
      fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: 'track it' } });
      fireEvent.click(screen.getByRole('button', { name: /send/i }));
      return screen.findByRole('alertdialog', { name: 'Allow this action?' });
    }

    it('shows the tool name and arguments and runs it only once allowed', async () => {
      setAnthropicApiKey('sk-ant-test');
      seedMcpTokens();
      confirmingTurn();
      const dialog = await send();
      expect(dialog).toHaveTextContent('track_train');
      expect(dialog).toHaveTextContent('C12345');
      expect(screen.queryByText('Tool was allowed.')).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Allow' }));
      expect(await screen.findByText('Tool was allowed.')).toBeInTheDocument();
      expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    });

    it('declines it on "Don\'t allow"', async () => {
      setAnthropicApiKey('sk-ant-test');
      seedMcpTokens();
      confirmingTurn();
      await send();
      fireEvent.click(screen.getByRole('button', { name: "Don't allow" }));
      expect(await screen.findByText('Tool was declined.')).toBeInTheDocument();
    });
  });

  describe('MCP sign-in (Connect / Reconnect)', () => {
    function send(text = 'hi') {
      fireEvent.change(screen.getByPlaceholderText(/ask about/i), { target: { value: text } });
      fireEvent.click(screen.getByRole('button', { name: /send/i }));
    }

    it('"Connect" starts a fresh sign-in against the configured MCP server', async () => {
      setAnthropicApiKey('sk-ant-test');
      mockStartMcpSignIn.mockReturnValue(new Promise(() => {}));
      renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
      send();
      fireEvent.click(await screen.findByRole('button', { name: 'Connect' }));
      expect(mockStartMcpSignIn).toHaveBeenCalledWith('https://mcp.example.com');
    });

    it('on a 401 from /mcp, drops the dead tokens and offers "Reconnect", which restarts sign-in', async () => {
      setAnthropicApiKey('sk-ant-test');
      seedMcpTokens();
      mockRunChatTurn.mockReturnValue(
        (async function* () {
          throw new StreamableHTTPError(401, 'Server returned 401 after successful authentication');
        })(),
      );
      mockStartMcpSignIn.mockReturnValue(new Promise(() => {}));
      renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
      send();
      const reconnect = await screen.findByRole('button', { name: 'Reconnect' });
      expect(screen.getByText(/has expired/i)).toBeInTheDocument();
      expect(localStorage.getItem('ds-mcp-oauth:tokens')).toBeNull();
      fireEvent.click(reconnect);
      expect(mockStartMcpSignIn).toHaveBeenCalledWith('https://mcp.example.com');
    });

    it('treats an SDK OAuth error (invalid_client surviving auth()) as needing Reconnect, not a tool error', async () => {
      const { InvalidClientError } = await import('@modelcontextprotocol/sdk/server/auth/errors.js');
      setAnthropicApiKey('sk-ant-test');
      seedMcpTokens();
      mockRunChatTurn.mockReturnValue(
        (async function* () {
          throw new InvalidClientError('Invalid client_id');
        })(),
      );
      renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
      send();
      expect(await screen.findByRole('button', { name: 'Reconnect' })).toBeInTheDocument();
      expect(screen.queryByText(/something went wrong answering that/i)).not.toBeInTheDocument();
    });

    it("treats the SDK's UnauthorizedError as needing Reconnect", async () => {
      const { UnauthorizedError } = await import('@modelcontextprotocol/sdk/client/auth.js');
      setAnthropicApiKey('sk-ant-test');
      seedMcpTokens();
      mockRunChatTurn.mockReturnValue(
        (async function* () {
          throw new UnauthorizedError();
        })(),
      );
      renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
      send();
      expect(await screen.findByRole('button', { name: 'Reconnect' })).toBeInTheDocument();
    });

    it('offers "Reconnect" (not Connect) when stored tokens belong to a registration the server has expired', async () => {
      setAnthropicApiKey('sk-ant-test');
      seedMcpTokens();
      localStorage.setItem(
        'ds-mcp-oauth:client-information',
        JSON.stringify({ client_id: 'old', client_id_issued_at: Math.floor(Date.now() / 1000) - 31 * 24 * 60 * 60 }),
      );
      renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
      send();
      expect(await screen.findByRole('button', { name: 'Reconnect' })).toBeInTheDocument();
      expect(mockRunChatTurn).not.toHaveBeenCalled();
      expect(localStorage.getItem('ds-mcp-oauth:client-information')).toBeNull();
      expect(localStorage.getItem('ds-mcp-oauth:tokens')).toBeNull();
    });

    it('on load, reports a sign-in that left for the authorization server and never came back', async () => {
      localStorage.setItem('ds-mcp-oauth:oauth-state', 'pending');
      renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
      expect(await screen.findByText(/last login to the rail data service didn.t finish/i)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Reconnect' })).toBeInTheDocument();
    });

    it('shows why sign-in could not start, with a way to try again', async () => {
      setAnthropicApiKey('sk-ant-test');
      mockStartMcpSignIn.mockRejectedValueOnce(new Error('HTTP 503 registering client'));
      renderWithMantine(<ChatPanel mcpServerUrl="https://mcp.example.com" />);
      send();
      fireEvent.click(await screen.findByRole('button', { name: 'Connect' }));
      expect(
        await screen.findByText(/couldn.t start logging in to the rail data service\. try again/i),
      ).toBeInTheDocument();
      mockStartMcpSignIn.mockReturnValue(new Promise(() => {}));
      fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
      expect(mockStartMcpSignIn).toHaveBeenCalledTimes(2);
    });
  });
});
