import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import JoinGroupPage from './page';
import { getGroupJoinPreview, getSession, ApiNotFoundError } from '@/lib/api';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getGroupJoinPreview: vi.fn(),
    getSession: vi.fn(),
  };
});

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/groups/join/tok123',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('JoinGroupPage', () => {
  it('shows an invalid-link message on ApiNotFoundError', async () => {
    vi.mocked(getGroupJoinPreview).mockRejectedValue(new ApiNotFoundError('404'));
    renderWithMantine(await JoinGroupPage({ params: Promise.resolve({ token: 'bad-token' }) }));
    expect(await screen.findByText('Invite link not found')).toBeInTheDocument();
  });

  it('shows a login link when the visitor is not authenticated', async () => {
    vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 3 });
    vi.mocked(getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });

    renderWithMantine(await JoinGroupPage({ params: Promise.resolve({ token: 'tok123' }) }));
    expect(screen.getByRole('heading', { name: 'Join Family?' })).toBeInTheDocument();
    expect(await screen.findByRole('link', { name: 'Log in to join Family' })).toBeInTheDocument();
  });

  it('shows the explicit Join button when already authenticated', async () => {
    vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 1 });
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    renderWithMantine(await JoinGroupPage({ params: Promise.resolve({ token: 'tok123' }) }));
    expect(screen.getByRole('button', { name: 'Join group' })).toBeInTheDocument();
    expect(screen.getByText('1 member already in this group.', { exact: false })).toBeInTheDocument();
  });
});
