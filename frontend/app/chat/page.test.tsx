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
    expect(screen.getByText(/Sign in to ask about live departures/)).toBeInTheDocument();
  });

  it('renders a "not available" message for a logged-in, non-allowlisted user -- not a 404', async () => {
    vi.mocked(api.getChatbotAccess).mockResolvedValue('forbidden');
    renderWithMantine(await ChatPage());
    expect(screen.getByText(/Not available for your account yet/)).toBeInTheDocument();
  });

  it('renders the ChatPanel for an allowed user', async () => {
    vi.mocked(api.getChatbotAccess).mockResolvedValue('allowed');
    renderWithMantine(await ChatPage());
    expect(screen.getByPlaceholderText(/next train/)).toBeInTheDocument();
  });
});
