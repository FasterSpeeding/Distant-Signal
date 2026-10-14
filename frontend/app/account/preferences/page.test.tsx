import { beforeEach, describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import PreferencesPage, { metadata } from './page';
import { getChatbotAccess, getSessionOrLoggedOut } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  getSessionOrLoggedOut: vi.fn(),
  getChatbotAccess: vi.fn(),
}));

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/account/preferences',
  useSearchParams: () => new URLSearchParams(''),
}));

const loggedOut = { authenticated: false, id: null, username: null, name: null } as const;
const loggedIn = { authenticated: true, id: 'user-1', username: null, name: 'Ada' } as const;

describe('PreferencesPage', () => {
  beforeEach(() => {
    vi.mocked(getChatbotAccess).mockResolvedValue({ status: 'unauthenticated' });
  });

  it('is titled Preferences', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedOut);
    renderWithMantine(await PreferencesPage());
    expect(screen.getByRole('heading', { level: 1, name: 'Preferences' })).toBeInTheDocument();
    expect(metadata.title).toBe('Preferences');
  });

  it('lets anyone pick a theme and a pride flag', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedOut);
    renderWithMantine(await PreferencesPage());
    expect(screen.getByRole('radiogroup', { name: 'Colour theme' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: /Pride flag/ })).toBeInTheDocument();
  });

  it('asks a logged-out visitor to log in for notifications, rather than offering the button', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedOut);
    renderWithMantine(await PreferencesPage());
    expect(screen.getByRole('link', { name: 'Log in to turn on notifications' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Enable notifications' })).toBeNull();
  });

  it('offers the notifications button when logged in', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    renderWithMantine(await PreferencesPage());
    expect(screen.getByRole('button', { name: 'Enable notifications' })).toBeInTheDocument();
  });

  it('shows the Claude API key only to accounts that can ask about trains', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(getChatbotAccess).mockResolvedValue({ status: 'forbidden' });
    const { unmount } = renderWithMantine(await PreferencesPage());
    expect(screen.queryByLabelText('Claude API key')).toBeNull();
    unmount();

    vi.mocked(getChatbotAccess).mockResolvedValue({ status: 'allowed', mode: 'group' });
    renderWithMantine(await PreferencesPage());
    expect(screen.getByRole('heading', { name: 'Ask about trains' })).toBeInTheDocument();
    expect(screen.getByLabelText('Claude API key')).toBeInTheDocument();
  });
});
