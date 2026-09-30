import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import AccountPage from './page';
import { getSessionOrLoggedOut } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  getSessionOrLoggedOut: vi.fn(),
}));

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/account',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('AccountPage', () => {
  it('asks an anonymous visitor to log in and offers no data actions', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue({
      authenticated: false,
      id: null,
      email: null,
      name: null,
    });
    renderWithMantine(await AccountPage());
    expect(screen.getByText(/Log in to download your data/)).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Download my data' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Delete my account' })).toBeNull();
  });

  it('offers a download link to the export endpoint and the delete flow when logged in', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'user-1',
      email: null,
      name: 'Ada',
    });
    renderWithMantine(await AccountPage());
    const link = screen.getByRole('link', { name: 'Download my data' });
    expect(link).toHaveAttribute('href', '/api/account/export');
    expect(link).toHaveAttribute('download');
    expect(screen.getByRole('button', { name: 'Delete my account' })).toBeInTheDocument();
  });

  it('states the retention periods, including the 14-day backup window', async () => {
    vi.mocked(getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'user-1',
      email: null,
      name: 'Ada',
    });
    renderWithMantine(await AccountPage());
    expect(screen.getByText(/18 months after the day of travel/)).toBeInTheDocument();
    expect(screen.getByText(/can stay in them for up to 14\s+days/)).toBeInTheDocument();
  });
});
