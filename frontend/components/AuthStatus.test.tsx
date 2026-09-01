import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AuthStatus } from './AuthStatus';
import type { SessionInfo } from '@/lib/types';

// LogoutButton calls useRouter() from next/navigation, which throws
// "invariant expected app router to be mounted" outside a real Next.js App
// Router tree (as in these unit tests) — same stub PinToggle.test.tsx uses.
const refresh = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh }),
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(''),
}));

const loggedOut: SessionInfo = { authenticated: false, id: null, email: null, name: null };

describe('AuthStatus', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refresh.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows a log in link when logged out', () => {
    renderWithMantine(<AuthStatus session={loggedOut} />);
    const link = screen.getByRole('link', { name: 'Log in' });
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2F');
  });

  it('does not show a log out button when logged out', () => {
    renderWithMantine(<AuthStatus session={loggedOut} />);
    expect(screen.queryByRole('button', { name: 'Log out' })).not.toBeInTheDocument();
  });

  it('shows the name when logged in with a name', () => {
    renderWithMantine(
      <AuthStatus session={{ authenticated: true, id: 'u1', email: 'a@b.com', name: 'Ada' }} />,
    );
    expect(screen.getByText('Ada')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Log out' })).toBeInTheDocument();
  });

  it('falls back to the email when logged in with no name', () => {
    renderWithMantine(
      <AuthStatus session={{ authenticated: true, id: 'u1', email: 'a@b.com', name: null }} />,
    );
    expect(screen.getByText('a@b.com')).toBeInTheDocument();
  });

  it('falls back to "Signed in" when both name and email are null', () => {
    renderWithMantine(<AuthStatus session={{ authenticated: true, id: 'u1', email: null, name: null }} />);
    expect(screen.getByText('Signed in')).toBeInTheDocument();
  });

  it('logging out posts to /api/auth/logout and refreshes the router', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(
      <AuthStatus session={{ authenticated: true, id: 'u1', email: 'a@b.com', name: 'Ada' }} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Log out' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/auth/logout', { method: 'POST' });
    });
    await waitFor(() => {
      expect(refresh).toHaveBeenCalled();
    });
  });
});
