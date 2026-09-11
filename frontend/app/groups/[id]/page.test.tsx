import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import GroupDetailPage from './page';
import {
  getGroup,
  getGroupMembers,
  getGroupTrains,
  getSession,
  ApiNotFoundError,
  ApiUnauthorizedError,
} from '@/lib/api';
import type { GroupMember, GroupRole, GroupTrain } from '@/lib/types';

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
      { userId: 'user-1', displayName: 'Alex', role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
      { userId: 'user-2', displayName: 'Sam', role: 'member', joinedAt: '2026-09-02T00:00:00Z' },
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
      { userId: 'user-1', displayName: 'Alex', role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
    ]);
    vi.mocked(getGroupTrains).mockResolvedValue([]);
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));
    expect(screen.queryByRole('button', { name: 'Remove' })).not.toBeInTheDocument();
  });

  it('shows a login prompt when the session has lapsed (ApiUnauthorizedError)', async () => {
    vi.mocked(getGroup).mockRejectedValue(new ApiUnauthorizedError('401'));
    renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));
    expect(await screen.findByRole('link', { name: 'Log in to view this group' })).toBeInTheDocument();
    expect(screen.queryByText('Group not found')).not.toBeInTheDocument();
  });

  /** Role-gating of the controls that the BACKEND gates more narrowly than
   * `canManage` -- each of these would otherwise be a button whose only
   * possible outcome is a 403 (promote, delete group) or a 404 (removing
   * someone else's shared train). */
  describe('role-gated controls', () => {
    const OWNER: GroupMember = {
      userId: 'user-owner',
      displayName: 'Olive',
      role: 'owner',
      joinedAt: '2026-09-01T00:00:00Z',
    };
    const ADMIN: GroupMember = {
      userId: 'user-admin',
      displayName: 'Adam',
      role: 'admin',
      joinedAt: '2026-09-02T00:00:00Z',
    };
    const PLAIN: GroupMember = {
      userId: 'user-plain',
      displayName: 'Priya',
      role: 'member',
      joinedAt: '2026-09-03T00:00:00Z',
    };

    function sharedTrain(addedBy: string, trainSubscriptionId: number): GroupTrain {
      return {
        trainSubscriptionId,
        pinOriginCrs: 'WOK',
        pinDestinationCrs: 'WAT',
        pinOriginName: 'Woking',
        pinDestinationName: 'London Waterloo',
        pinScheduledDeparture: '2026-09-11T08:00:00Z',
        serviceDate: '2026-09-11',
        resolutionStatus: 'resolved',
        trainUid: 'A12345',
        status: 'en_route',
        delayMinutes: null,
        customName: `Train ${trainSubscriptionId}`,
        addedBy,
        addedByName: addedBy,
      };
    }

    /** Renders the page as `viewer`, with `viewerRole` as their role on the
     * group -- the two always agree in reality, so they're set together. */
    async function renderAs(viewer: GroupMember, viewerRole: GroupRole, trains: GroupTrain[] = []) {
      vi.mocked(getGroup).mockResolvedValue({
        id: 'grp-1',
        name: 'Family',
        ownerId: OWNER.userId,
        ownerName: OWNER.displayName,
        memberCount: 3,
        role: viewerRole,
        inviteLink: null,
      });
      vi.mocked(getGroupMembers).mockResolvedValue([OWNER, ADMIN, PLAIN]);
      vi.mocked(getGroupTrains).mockResolvedValue(trains);
      vi.mocked(getSession).mockResolvedValue({
        authenticated: true,
        id: viewer.userId,
        email: null,
        name: viewer.displayName,
      });
      renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));
    }

    it('an owner viewer sees "Promote to admin" on a plain member row', async () => {
      await renderAs(OWNER, 'owner');
      expect(screen.getByRole('button', { name: 'Promote to admin' })).toBeInTheDocument();
    });

    it('an admin viewer does NOT see "Promote to admin" (the backend is owner-only)', async () => {
      await renderAs(ADMIN, 'admin');
      expect(screen.queryByRole('button', { name: 'Promote to admin' })).not.toBeInTheDocument();
      // ...but the admin still has the controls the backend DOES grant
      // them, so this isn't just "an admin sees nothing".
      expect(screen.getAllByRole('button', { name: 'Remove' }).length).toBeGreaterThan(0);
    });

    it('a plain member viewer does NOT see "Remove from group" on someone else\'s shared train', async () => {
      await renderAs(PLAIN, 'member', [sharedTrain(ADMIN.userId, 41)]);
      expect(screen.queryByRole('button', { name: 'Remove from group' })).not.toBeInTheDocument();
    });

    it("a plain member viewer DOES see \"Remove from group\" on their own shared train", async () => {
      await renderAs(PLAIN, 'member', [sharedTrain(PLAIN.userId, 42)]);
      expect(screen.getByRole('button', { name: 'Remove from group' })).toBeInTheDocument();
    });

    it('an admin viewer sees "Remove from group" on someone else\'s shared train', async () => {
      await renderAs(ADMIN, 'admin', [sharedTrain(PLAIN.userId, 43)]);
      expect(screen.getByRole('button', { name: 'Remove from group' })).toBeInTheDocument();
    });

    it('only an owner sees "Delete group"; an admin and a plain member see "Rename" / neither', async () => {
      await renderAs(OWNER, 'owner');
      expect(screen.getByRole('button', { name: 'Rename' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Delete group' })).toBeInTheDocument();
    });

    it('an admin viewer sees "Rename" but not "Delete group"', async () => {
      await renderAs(ADMIN, 'admin');
      expect(screen.getByRole('button', { name: 'Rename' })).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Delete group' })).not.toBeInTheDocument();
    });

    it('a plain member viewer sees neither "Rename" nor "Delete group"', async () => {
      await renderAs(PLAIN, 'member');
      expect(screen.queryByRole('button', { name: 'Rename' })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Delete group' })).not.toBeInTheDocument();
      // The self-service control every member always has stays put.
      expect(screen.getByRole('button', { name: 'Leave group' })).toBeInTheDocument();
    });

    /** `LeaveGroupButton`'s confirm copy must warn about the WHOLE group
     * being deleted when the viewer is the sole owner (`remove_member`'s
     * sole-owner-leaves branch deletes the group, not just their own
     * membership row) -- and must NOT show that warning otherwise, even for
     * an owner who isn't alone. */
    it('warns the sole owner that leaving deletes the whole group', async () => {
      vi.mocked(getGroup).mockResolvedValue({
        id: 'grp-1',
        name: 'Family',
        ownerId: OWNER.userId,
        ownerName: OWNER.displayName,
        memberCount: 1,
        role: 'owner',
        inviteLink: null,
      });
      vi.mocked(getGroupMembers).mockResolvedValue([OWNER]);
      vi.mocked(getGroupTrains).mockResolvedValue([]);
      vi.mocked(getSession).mockResolvedValue({
        authenticated: true,
        id: OWNER.userId,
        email: null,
        name: OWNER.displayName,
      });
      renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));

      fireEvent.click(screen.getByRole('button', { name: 'Leave group' }));
      await waitFor(() => screen.getByText(/delete it for good/));
    });

    it('does not warn about deleting the group for an owner with other members', async () => {
      await renderAs(OWNER, 'owner');

      fireEvent.click(screen.getByRole('button', { name: 'Leave group' }));
      await waitFor(() => screen.getByText(/lose access to every train shared/));
      expect(screen.queryByText(/delete it for good/)).not.toBeInTheDocument();
    });
  });
});
