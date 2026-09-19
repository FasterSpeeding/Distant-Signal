import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import JoinGroupPage, { generateMetadata } from './page';
import { getGroup, getGroupJoinPreview, getSession, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getGroup: vi.fn(),
    getGroupJoinPreview: vi.fn(),
    getSession: vi.fn(),
  };
});

// Every authenticated-visitor test below relies on the membership probe
// (`getGroup`); default it to "not a member yet" (an `ApiNotFoundError`,
// matching `get_group_detail`'s own contract) so tests that don't care
// about the already-member branch see the ordinary `JoinGroupButton` path
// unchanged. Tests that DO care override this per-case.
beforeEach(() => {
  vi.mocked(getGroup).mockReset().mockRejectedValue(new ApiNotFoundError('not a member'));
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

  // Review §2.16: the anonymous CTA on this page -- the join step *is* the
  // login step here -- used to be an underlined text link where the
  // authenticated branch (the case above) shows a filled `Button`. It's now
  // promoted to the same filled treatment.
  it('renders the anonymous login action as a filled button, not a plain text link', async () => {
    vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 3 });
    vi.mocked(getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });

    renderWithMantine(await JoinGroupPage({ params: Promise.resolve({ token: 'tok123' }) }));
    expect(await screen.findByRole('button', { name: 'Log in to join Family' })).toBeInTheDocument();
  });

  it('shows the explicit Join button when already authenticated', async () => {
    vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 1 });
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    renderWithMantine(await JoinGroupPage({ params: Promise.resolve({ token: 'tok123' }) }));
    expect(screen.getByRole('button', { name: 'Join group' })).toBeInTheDocument();
    expect(screen.getByText('1 member already in this group.', { exact: false })).toBeInTheDocument();
  });

  // Review §4.6 / §3.2.3: an already-authenticated visitor who's already a
  // member -- most often the group's own owner testing their own invite
  // link -- gets "You're already in {name}" and an "Open group" action
  // instead of a confusing "Join group" offer.
  describe('an already-authenticated member', () => {
    it('shows "You\'re already in" and an Open group link instead of Join', async () => {
      vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 3 });
      vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });
      vi.mocked(getGroup).mockResolvedValue({
        id: 'grp-1',
        name: 'Family',
        ownerId: 'user-1',
        ownerName: 'Alex',
        ownerTag: null,
        memberCount: 3,
        role: 'owner',
        inviteLink: null,
      });

      renderWithMantine(await JoinGroupPage({ params: Promise.resolve({ token: 'tok123' }) }));
      expect(screen.getByRole('heading', { name: "You're already in Family" })).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Open group' })).toHaveAttribute('href', '/groups/grp-1');
      expect(screen.queryByRole('button', { name: 'Join group' })).not.toBeInTheDocument();
      // The "Join Family?" framing and its "Joining lets everyone..."
      // explanation belong to the not-yet-a-member branch only -- neither
      // makes sense paired with "you're already in".
      expect(screen.queryByRole('heading', { name: 'Join Family?' })).not.toBeInTheDocument();
      expect(screen.queryByText(/Joining lets everyone/)).not.toBeInTheDocument();
    });

    it('does not probe membership for an anonymous visitor', async () => {
      vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 3 });
      vi.mocked(getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });

      renderWithMantine(await JoinGroupPage({ params: Promise.resolve({ token: 'tok123' }) }));
      expect(vi.mocked(getGroup)).not.toHaveBeenCalled();
      expect(screen.getByRole('button', { name: 'Log in to join Family' })).toBeInTheDocument();
    });

    it('falls back to the ordinary Join button on a lapsed-session race (ApiUnauthorizedError)', async () => {
      vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 3 });
      vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });
      vi.mocked(getGroup).mockRejectedValue(new ApiUnauthorizedError('401'));

      renderWithMantine(await JoinGroupPage({ params: Promise.resolve({ token: 'tok123' }) }));
      expect(screen.getByRole('button', { name: 'Join group' })).toBeInTheDocument();
    });

    it('propagates an unexpected error from the membership probe', async () => {
      vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 3 });
      vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });
      vi.mocked(getGroup).mockRejectedValue(new Error('boom'));

      await expect(JoinGroupPage({ params: Promise.resolve({ token: 'tok123' }) })).rejects.toThrow('boom');
    });
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
