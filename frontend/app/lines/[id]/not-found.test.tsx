import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import LineNotFound from './not-found';
import * as api from '@/lib/api';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getSession: vi.fn() };
});
// LoginLink calls usePathname()/useSearchParams() -- same workaround every
// other test rendering it uses.
vi.mock('next/navigation', () => ({
  usePathname: () => '/lines/some-id/edit',
  useSearchParams: () => new URLSearchParams(''),
}));

function session(authenticated: boolean) {
  return { authenticated, id: authenticated ? 'u1' : null, email: authenticated ? 'a@b.com' : null, name: null };
}

describe('LineNotFound', () => {
  // Task 3.4.10: "Back to your dashboard" assumed a home page an anonymous
  // visitor doesn't have -- neutral wording works for both.
  it('offers a neutral "Go to the home page" link, not "Back to your dashboard"', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    renderWithMantine(await LineNotFound());

    expect(screen.getByRole('link', { name: 'Go to the home page' })).toHaveAttribute('href', '/');
    expect(screen.queryByText('Back to your dashboard')).not.toBeInTheDocument();
  });

  it('offers a "Log in" link to an anonymous visitor', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(false));
    renderWithMantine(await LineNotFound());

    expect(screen.getByRole('link', { name: 'Log in' })).toBeInTheDocument();
  });

  it('does not offer a "Log in" link to a signed-in visitor', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    renderWithMantine(await LineNotFound());

    expect(screen.queryByRole('link', { name: 'Log in' })).not.toBeInTheDocument();
  });

  // Failing closed: an unreachable session check must not hide the login
  // link from a visitor who may well be genuinely logged out.
  it('shows the "Log in" link when the session check itself fails', async () => {
    vi.mocked(api.getSession).mockRejectedValue(new Error('connect ECONNREFUSED'));
    renderWithMantine(await LineNotFound());

    expect(screen.getByRole('link', { name: 'Log in' })).toBeInTheDocument();
  });

  it('still links to "Browse all lines"', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    renderWithMantine(await LineNotFound());

    expect(screen.getByRole('link', { name: 'Browse all lines' })).toHaveAttribute('href', '/lines');
  });
});
