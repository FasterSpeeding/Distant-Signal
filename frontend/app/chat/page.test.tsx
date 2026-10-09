import { afterEach, describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import ChatPage from './page';
import * as api from '@/lib/api';

vi.mock('@/lib/api');
// The not-logged-in state renders AutoOpenLoginPrompt -> LoginPromptModal,
// which calls useLoginHref() (usePathname()/useSearchParams() under the
// hood) -- same stub track/mine/page.test.tsx/AuthStatus.test.tsx use for
// the same reason (useRouter()/usePathname()/useSearchParams() throw
// outside an app router context in a plain component test).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/chat',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('ChatPage', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('renders a login prompt for an unauthenticated visitor', async () => {
    vi.mocked(api.getChatbotAccess).mockResolvedValue({ status: 'unauthenticated' });
    renderWithMantine(await ChatPage());
    // Two matches now: the server-rendered LoginLink sentence and the
    // AutoOpenLoginPrompt modal's own copy of it -- see the dedicated
    // LoginLink assertion below for the inline one specifically.
    expect(screen.getAllByText(/Log in to ask about live departures/).length).toBeGreaterThan(0);
  });

  it('unauthenticated: also renders a server-rendered LoginLink, not just the client-only modal', async () => {
    vi.mocked(api.getChatbotAccess).mockResolvedValue({ status: 'unauthenticated' });
    renderWithMantine(await ChatPage());
    const link = screen.getByRole('link', {
      name: 'Log in to ask about live departures, disruptions and journeys',
    });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2Fchat');
  });

  it('renders a "not available" message for a logged-in, non-allowlisted user -- not a 404', async () => {
    vi.mocked(api.getChatbotAccess).mockResolvedValue({ status: 'forbidden' });
    renderWithMantine(await ChatPage());
    expect(screen.getByText(/Not available for your account yet/)).toBeInTheDocument();
  });

  // Review §3.1.2 (F8): a forbidden user gets a next step -- the MCP
  // server has its own access group, separate from the chat allowlist.
  it('offers the add-to-your-own-assistant section to a forbidden user when the MCP URL is set', async () => {
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
    vi.mocked(api.getChatbotAccess).mockResolvedValue({ status: 'forbidden' });
    renderWithMantine(await ChatPage());
    expect(screen.getByRole('heading', { name: 'Use Distant Signal in your own assistant' })).toBeInTheDocument();
    expect(screen.getByLabelText('MCP server URL')).toHaveValue('https://mcp.example.com/mcp');
    expect(screen.getByText(/Only accounts that have been given access/)).toBeInTheDocument();
  });

  it('forbidden with no MCP URL: no add-to-assistant section and no link to nowhere', async () => {
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', '');
    vi.mocked(api.getChatbotAccess).mockResolvedValue({ status: 'forbidden' });
    renderWithMantine(await ChatPage());
    expect(screen.getByText(/open to a small group of accounts/)).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Use Distant Signal in your own assistant' })).not.toBeInTheDocument();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('renders the ChatPanel for an allowed user', async () => {
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
    vi.mocked(api.getChatbotAccess).mockResolvedValue({ status: 'allowed', mode: 'group' });
    renderWithMantine(await ChatPage());
    expect(screen.getByPlaceholderText(/next train/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Use Distant Signal in your own assistant' })).toBeInTheDocument();
  });

  it('group mode: keeps the "only accounts that have been given access" note for an allowed user', async () => {
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
    vi.mocked(api.getChatbotAccess).mockResolvedValue({ status: 'allowed', mode: 'group' });
    renderWithMantine(await ChatPage());
    expect(screen.getByText(/Only accounts that have been given access/)).toBeInTheDocument();
  });

  it('authenticated mode: any logged-in user gets the ChatPanel and no access-restriction notes', async () => {
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
    vi.mocked(api.getChatbotAccess).mockResolvedValue({ status: 'allowed', mode: 'authenticated' });
    renderWithMantine(await ChatPage());
    expect(screen.getByPlaceholderText(/next train/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Use Distant Signal in your own assistant' })).toBeInTheDocument();
    expect(screen.queryByText(/Only accounts that have been given access/)).not.toBeInTheDocument();
    expect(screen.queryByText(/allowlist/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Not available for your account/)).not.toBeInTheDocument();
  });

  // FE-2: the MCP URL is read at request time on the server.
  it('says chat is not configured, instead of mounting ChatPanel, when the MCP URL is unset', async () => {
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', '');
    vi.mocked(api.getChatbotAccess).mockResolvedValue({ status: 'allowed', mode: 'group' });
    renderWithMantine(await ChatPage());
    expect(screen.getByText(/isn.t available on this site/)).toBeInTheDocument();
    expect(screen.queryByPlaceholderText(/next train/)).not.toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Use Distant Signal in your own assistant' })).not.toBeInTheDocument();
  });
});
