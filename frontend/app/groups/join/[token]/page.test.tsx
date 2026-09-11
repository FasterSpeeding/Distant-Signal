import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import JoinGroupPage, { generateMetadata } from './page';
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
  notFound: vi.fn(),
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

describe('generateMetadata', () => {
  it('titles the page with the group name and describes the member count', async () => {
    vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 3 });
    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'tok123' }) });
    expect(metadata.title).toBe('Join Family — Distant Signal');
    expect(metadata.description).toBe(
      '3 members already in Family. Follow this link to join and share tracked trains with the group.',
    );
    expect(metadata.openGraph).toMatchObject({ title: 'Join Family — Distant Signal', type: 'website' });
    expect(metadata.twitter).toMatchObject({ card: 'summary', title: 'Join Family — Distant Signal' });
  });

  it('uses singular "member" for a group of one', async () => {
    vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Solo', memberCount: 1 });
    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'tok123' }) });
    expect(metadata.description).toBe(
      '1 member already in Solo. Follow this link to join and share tracked trains with the group.',
    );
  });

  it('calls notFound() on ApiNotFoundError, matching the page component', async () => {
    vi.mocked(getGroupJoinPreview).mockRejectedValue(new ApiNotFoundError('not found'));
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();
    await expect(generateMetadata({ params: Promise.resolve({ token: 'bad-token' }) })).rejects.toThrow();
    expect(notFound).toHaveBeenCalled();
  });
});
