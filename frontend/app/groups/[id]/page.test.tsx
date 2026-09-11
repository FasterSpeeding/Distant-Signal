import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import GroupDetailPage from './page';
import { getGroup, getGroupMembers, getGroupTrains, getSession, ApiNotFoundError } from '@/lib/api';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getGroup: vi.fn(),
    getGroupMembers: vi.fn(),
    getGroupTrains: vi.fn(),
    getSession: vi.fn(),
  };
});

vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn(), push: vi.fn() }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('GroupDetailPage', () => {
  it('shows a not-found message on ApiNotFoundError', async () => {
    vi.mocked(getGroup).mockRejectedValue(new ApiNotFoundError('404'));
    renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-missing' }) }));
    expect(await screen.findByText('Group not found')).toBeInTheDocument();
  });

  it('renders the group name, members, and shared trains', async () => {
    vi.mocked(getGroup).mockResolvedValue({
      id: 'grp-1',
      name: 'Family',
      ownerId: 'user-1',
      ownerName: 'Alex',
      memberCount: 2,
      role: 'owner',
      inviteLink: { token: 'tok', expiresAt: '2026-09-18T00:00:00Z' },
    });
    vi.mocked(getGroupMembers).mockResolvedValue([
      { userId: 'user-1', name: 'Alex', email: null, role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
      { userId: 'user-2', name: 'Sam', email: null, role: 'member', joinedAt: '2026-09-02T00:00:00Z' },
    ]);
    vi.mocked(getGroupTrains).mockResolvedValue([
      {
        trainSubscriptionId: 42,
        pinOriginCrs: 'WOK',
        pinDestinationCrs: 'WAT',
        pinOriginName: 'Woking',
        pinDestinationName: 'London Waterloo',
        pinScheduledDeparture: '2026-09-11T08:00:00Z',
        serviceDate: '2026-09-11',
        resolutionStatus: 'resolved',
        trainUid: 'A12345',
        status: 'en_route',
        delayMinutes: 5,
        customName: null,
        addedBy: 'user-2',
        addedByName: 'Sam',
      },
    ]);
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));

    expect(screen.getByRole('heading', { name: 'Family' })).toBeInTheDocument();
    expect(screen.getByText('Alex')).toBeInTheDocument();
    expect(screen.getByText('Sam')).toBeInTheDocument();
    expect(screen.getByText(/Shared by Sam/)).toBeInTheDocument();
  });

  it('never renders a Remove button for the owner row', async () => {
    vi.mocked(getGroup).mockResolvedValue({
      id: 'grp-1',
      name: 'Family',
      ownerId: 'user-1',
      ownerName: 'Alex',
      memberCount: 1,
      role: 'owner',
      inviteLink: null,
    });
    vi.mocked(getGroupMembers).mockResolvedValue([
      { userId: 'user-1', name: 'Alex', email: null, role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
    ]);
    vi.mocked(getGroupTrains).mockResolvedValue([]);
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));
    expect(screen.queryByRole('button', { name: 'Remove' })).not.toBeInTheDocument();
  });
});
