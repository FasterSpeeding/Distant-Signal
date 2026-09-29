import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AddMcpServerLinks } from './AddMcpServerLinks';

// jsdom has no `navigator.clipboard` -- same stub as connect-claude's test.
function stubClipboard(writeText: ReturnType<typeof vi.fn>) {
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, writable: true, configurable: true });
}

const PUBLIC_URL = 'https://ds-mcp.example.com';
const ENDPOINT = `${PUBLIC_URL}/mcp`;

/** The live-region line under a copy field. */
function statusFor(fieldLabel: string): HTMLElement {
  const field = screen.getByLabelText(fieldLabel).closest<HTMLElement>('[data-copy-field]');
  if (!field) throw new Error(`no copy field for ${fieldLabel}`);
  return within(field).getByRole('status');
}

describe('AddMcpServerLinks', () => {
  afterEach(() => {
    // @ts-expect-error -- removing the stub TS believes is always present.
    delete navigator.clipboard;
  });

  it('renders a labelled section with the MCP endpoint built from the runtime URL', () => {
    renderWithMantine(<AddMcpServerLinks mcpPublicUrl={`${PUBLIC_URL}/`} />);
    const section = screen.getByRole('region', { name: 'Use Distant Signal in your own assistant' });
    expect(within(section).getByLabelText('MCP server URL')).toHaveValue(ENDPOINT);
    expect(screen.getByText(/Only accounts that have been given access/)).toBeInTheDocument();
  });

  it('links to the Cursor and VS Code installers with the endpoint', () => {
    renderWithMantine(<AddMcpServerLinks mcpPublicUrl={PUBLIC_URL} />);
    const cursor = screen.getByRole('link', { name: 'Add to Cursor' }).getAttribute('href') ?? '';
    expect(cursor).toMatch(/^cursor:\/\/anysphere\.cursor-deeplink\/mcp\/install\?name=distant-signal&config=/);
    expect(JSON.parse(atob(new URL(cursor).searchParams.get('config') ?? ''))).toEqual({ url: ENDPOINT });

    expect(screen.getByRole('link', { name: 'Add to VS Code' })).toHaveAttribute(
      'href',
      `vscode:mcp/install?${encodeURIComponent(JSON.stringify({ name: 'distant-signal', type: 'http', url: ENDPOINT }))}`,
    );
  });

  it('shows the Claude Code and Codex commands, the Claude.ai connectors link and ChatGPT steps', () => {
    renderWithMantine(<AddMcpServerLinks mcpPublicUrl={PUBLIC_URL} />);
    expect(screen.getByLabelText('Claude Code command')).toHaveValue(
      `claude mcp add --transport http distant-signal ${ENDPOINT}`,
    );
    expect(screen.getByLabelText('Codex CLI command')).toHaveValue(`codex mcp add distant-signal --url ${ENDPOINT}`);
    expect(screen.getByRole('link', { name: 'Customize > Connectors' })).toHaveAttribute(
      'href',
      'https://claude.ai/customize/connectors',
    );
    expect(screen.getByRole('heading', { name: 'ChatGPT' })).toBeInTheDocument();
  });

  it('shows the Gemini CLI command and how to sign in there', () => {
    renderWithMantine(<AddMcpServerLinks mcpPublicUrl={PUBLIC_URL} />);
    expect(screen.getByLabelText('Gemini CLI command')).toHaveValue(
      `gemini mcp add --transport http distant-signal ${ENDPOINT}`,
    );
    expect(screen.getByText('Then run /mcp auth distant-signal in Gemini CLI to sign in.')).toBeInTheDocument();
  });

  it('links to /connect-claude from the Claude.ai steps', () => {
    renderWithMantine(<AddMcpServerLinks mcpPublicUrl={PUBLIC_URL} />);
    expect(screen.getByRole('link', { name: 'More about connecting Claude' })).toHaveAttribute(
      'href',
      '/connect-claude',
    );
  });

  it('has a collapsed troubleshooting note for the unregistered redirect_uri error', () => {
    const { container } = renderWithMantine(<AddMcpServerLinks mcpPublicUrl={PUBLIC_URL} />);
    const details = container.querySelector('details');
    expect(details).not.toBeNull();
    expect(details).not.toHaveAttribute('open');
    expect(within(details as HTMLElement).getByText(/unregistered redirect_uri/)).toBeInTheDocument();
    expect(within(details as HTMLElement).getByText(/localhost instead of 127\.0\.0\.1/)).toBeInTheDocument();
    expect(
      within(details as HTMLElement).getByText(/Remove Distant Signal from the assistant, add it again/),
    ).toBeInTheDocument();
  });

  it('copies the URL and announces it in a polite live region', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    stubClipboard(writeText);
    renderWithMantine(<AddMcpServerLinks mcpPublicUrl={PUBLIC_URL} />);
    fireEvent.click(screen.getByRole('button', { name: 'Copy MCP server URL' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(ENDPOINT));
    const status = statusFor('MCP server URL');
    expect(status).toHaveAttribute('aria-live', 'polite');
    await waitFor(() => expect(status).toHaveTextContent('Copied.'));
  });

  it('copies the Claude Code command', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    stubClipboard(writeText);
    renderWithMantine(<AddMcpServerLinks mcpPublicUrl={PUBLIC_URL} />);
    fireEvent.click(screen.getByRole('button', { name: 'Copy Claude Code command' }));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(`claude mcp add --transport http distant-signal ${ENDPOINT}`),
    );
    await waitFor(() => expect(statusFor('Claude Code command')).toHaveTextContent('Copied.'));
  });

  it('says so, visibly and in the live region, when the clipboard is unavailable', async () => {
    renderWithMantine(<AddMcpServerLinks mcpPublicUrl={PUBLIC_URL} />);
    fireEvent.click(screen.getByRole('button', { name: 'Copy MCP server URL' }));
    await waitFor(() => expect(statusFor('MCP server URL')).toHaveTextContent(/Couldn’t copy/));
  });
});
