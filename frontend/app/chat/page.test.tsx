import { afterEach, describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import ChatPage from './page';
import * as api from '@/lib/api';

vi.mock('@/lib/api');
// The not-logged-in state renders AutoOpenLoginPrompt -> LoginPromptModal,
// which calls useLoginHref() (usePathname()/useSearchParams() under the
// hood) -- same stub journeys/page.test.tsx/AuthStatus.test.tsx use for
// the same reason (useRouter()/usePathname()/useSearchParams() throw
// outside an app router context in a plain component test).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/chat',
  useSearchParams: () => new URLSearchParams(''),
}));

const LOGGED_OUT = { authenticated: false, id: null, name: null, username: null };
const LOGGED_IN = { authenticated: true, id: 'u1', username: 'ada', name: 'Ada' };

describe('ChatPage', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('renders a login prompt for an unauthenticated visitor', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_OUT);
    renderWithMantine(await ChatPage());
    // Two matches now: the server-rendered LoginControl sentence and the
    // AutoOpenLoginPrompt modal's own copy of it -- see the dedicated
    // LoginControl assertion below for the inline one specifically.
    expect(screen.getAllByText(/Log in to ask about live departures/).length).toBeGreaterThan(0);
    expect(screen.queryByPlaceholderText(/next train/)).not.toBeInTheDocument();
  });

  it('unauthenticated: also renders a server-rendered LoginControl, not just the client-only modal', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_OUT);
    renderWithMantine(await ChatPage());
    const link = screen.getByRole('link', {
      name: 'Log in to ask about live departures, disruptions and journeys',
    });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2Fchat');
  });

  // The chat has no access group any more: any account gets it, with no
  // "given access" or "small group of accounts" wording anywhere.
  it('renders the ChatPanel and the add-to-your-own-assistant section for any logged-in user', async () => {
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    renderWithMantine(await ChatPage());
    expect(screen.getByPlaceholderText(/next train/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Use Distant Signal in your own assistant' })).toBeInTheDocument();
    expect(screen.getByLabelText('MCP server URL')).toHaveValue('https://mcp.example.com/mcp');
    expect(screen.queryByText(/given access/)).not.toBeInTheDocument();
    expect(screen.queryByText(/small group of accounts/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Not available for your account/)).not.toBeInTheDocument();
  });

  // FE-2: the MCP URL is read at request time on the server.
  it('says chat is not configured, instead of mounting ChatPanel, when the MCP URL is unset', async () => {
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', '');
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    renderWithMantine(await ChatPage());
    expect(screen.getByText(/isn.t available on this site/)).toBeInTheDocument();
    expect(screen.queryByPlaceholderText(/next train/)).not.toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Use Distant Signal in your own assistant' })).not.toBeInTheDocument();
  });
});
