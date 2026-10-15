import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import PreferencesPage, { metadata } from './page';
import { getConnectedApps, getSessionOrLoggedOut } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  getSessionOrLoggedOut: vi.fn(),
  getConnectedApps: vi.fn(async () => ({ grants: [], available: [], recentDeletions: [] })),
}));

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/account/preferences',
  useSearchParams: () => new URLSearchParams(''),
}));

const loggedOut = { authenticated: false, id: null, username: null, name: null } as const;
const loggedIn = { authenticated: true, id: 'user-1', username: null, name: 'Ada' } as const;

describe('PreferencesPage', () => {
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

  it('asks a logged-out visitor to log in for the account sections, rather than hiding them or redirecting', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedOut);
    renderWithMantine(await PreferencesPage());
    expect(screen.getByRole('heading', { level: 1, name: 'Preferences' })).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Account settings' })).toBeInTheDocument();
    const login = screen.getByRole('link', { name: 'Log in to manage notifications and more' });
    expect(login.getAttribute('href')).toContain('return_to=%2Faccount%2Fpreferences');
    expect(screen.queryByRole('button', { name: 'Enable notifications' })).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Notifications' })).toBeNull();
    expect(screen.queryByLabelText('Claude API key')).toBeNull();
  });

  it('offers the notifications button when logged in', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    renderWithMantine(await PreferencesPage());
    expect(screen.getByRole('button', { name: 'Enable notifications' })).toBeInTheDocument();
    expect(screen.getByRole('radiogroup', { name: 'Colour theme' })).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /^Log in/ })).toBeNull();
  });

  it('shows the Claude API key to every logged-in account, and not to a logged-out visitor', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedOut);
    const { unmount } = renderWithMantine(await PreferencesPage());
    expect(screen.queryByLabelText('Claude API key')).toBeNull();
    unmount();

    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    renderWithMantine(await PreferencesPage());
    expect(screen.getByRole('heading', { name: 'Ask about trains' })).toBeInTheDocument();
    expect(screen.getByLabelText('Claude API key')).toBeInTheDocument();
  });

  it('lists connected apps for a logged-in account, below the Claude key', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    renderWithMantine(await PreferencesPage());
    const claude = screen.getByRole('heading', { name: 'Ask about trains' });
    const apps = screen.getByRole('heading', { level: 2, name: 'Connected apps' });
    expect(claude.compareDocumentPosition(apps) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getByText('No apps are connected.')).toBeInTheDocument();
  });

  it('leaves Connected apps out for a logged-out visitor, or when it cannot load', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedOut);
    const { unmount } = renderWithMantine(await PreferencesPage());
    expect(screen.queryByRole('heading', { name: 'Connected apps' })).toBeNull();
    unmount();
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(getConnectedApps).mockRejectedValueOnce(new Error('down'));
    renderWithMantine(await PreferencesPage());
    expect(screen.queryByRole('heading', { name: 'Connected apps' })).toBeNull();
    expect(screen.getByLabelText('Claude API key')).toBeInTheDocument();
  });
});
