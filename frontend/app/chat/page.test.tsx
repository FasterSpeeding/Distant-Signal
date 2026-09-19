import { describe, it, expect, vi } from 'vitest';
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
  it('renders a login prompt for an unauthenticated visitor', async () => {
    vi.mocked(api.getChatbotAccess).mockResolvedValue('unauthenticated');
    renderWithMantine(await ChatPage());
    // Two matches now: the server-rendered LoginLink sentence and the
    // AutoOpenLoginPrompt modal's own copy of it -- see the dedicated
    // LoginLink assertion below for the inline one specifically.
    expect(screen.getAllByText(/Sign in to ask about live departures/).length).toBeGreaterThan(0);
  });

  it('unauthenticated: also renders a server-rendered LoginLink, not just the client-only modal', async () => {
    vi.mocked(api.getChatbotAccess).mockResolvedValue('unauthenticated');
    renderWithMantine(await ChatPage());
    const link = screen.getByRole('link', {
      name: 'Sign in to ask about live departures, disruptions and journeys',
    });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2Fchat');
  });

  it('renders a "not available" message for a logged-in, non-allowlisted user -- not a 404', async () => {
    vi.mocked(api.getChatbotAccess).mockResolvedValue('forbidden');
    renderWithMantine(await ChatPage());
    expect(screen.getByText(/Not available for your account yet/)).toBeInTheDocument();
  });

  // Review §3.1.2 (F8): this dead end used to have no explanation and no
  // next step -- /connect-claude works for every logged-in user regardless
  // of the chatbot allowlist, so it's a real next step.
  it('offers /connect-claude as a next step for a forbidden (non-allowlisted) user', async () => {
    vi.mocked(api.getChatbotAccess).mockResolvedValue('forbidden');
    renderWithMantine(await ChatPage());
    expect(screen.getByRole('link', { name: 'connecting Claude to Distant Signal' })).toHaveAttribute(
      'href',
      '/connect-claude',
    );
  });

  it('renders the ChatPanel for an allowed user', async () => {
    vi.mocked(api.getChatbotAccess).mockResolvedValue('allowed');
    renderWithMantine(await ChatPage());
    expect(screen.getByPlaceholderText(/next train/)).toBeInTheDocument();
  });
});
