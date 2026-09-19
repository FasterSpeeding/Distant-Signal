import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { expectShrinkGuarded } from '@/test/shrinkGuard';
import GroupDetailPage from './page';
import {
  getGroup,
  getGroupCustomLines,
  getGroupMembers,
  getGroupTrains,
  getLineStatus,
  getSession,
  ApiNotFoundError,
  ApiUnauthorizedError,
} from '@/lib/api';
import type {
  GroupCustomLine,
  GroupMember,
  GroupRole,
  GroupTrain,
  LineStatusReport,
} from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getGroup: vi.fn(),
    getGroupCustomLines: vi.fn(),
    getGroupMembers: vi.fn(),
    getGroupTrains: vi.fn(),
    getLineStatus: vi.fn(),
    getSession: vi.fn(),
  };
});

// Every test that gets past the group fetch renders the shared-custom-lines
// section, so both of its calls need a default; individual tests below
// override what they care about.
beforeEach(() => {
  vi.mocked(getGroupCustomLines).mockResolvedValue([]);
  vi.mocked(getLineStatus).mockResolvedValue([]);
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
      ownerTag: null,
      memberCount: 2,
      role: 'owner',
      inviteLink: { token: 'tok', expiresAt: '2026-09-18T00:00:00Z' },
    });
    vi.mocked(getGroupMembers).mockResolvedValue([
      { userId: 'user-1', displayName: 'Alex', displayTag: null, role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
      { userId: 'user-2', displayName: 'Sam', displayTag: null, role: 'member', joinedAt: '2026-09-02T00:00:00Z' },
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
        addedByTag: null,
      },
    ]);
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));

    expect(screen.getByRole('heading', { name: 'Family' })).toBeInTheDocument();
    expect(screen.getByText('Alex')).toBeInTheDocument();
    expect(screen.getByText('Sam')).toBeInTheDocument();
    expect(screen.getByText(/Shared by Sam/)).toBeInTheDocument();
    // Review §2.9: this card used to print the raw `status` enum token
    // ("en_route") verbatim in the subtitle. It now renders through the
    // same `TrackedTrainStatusBadge` `/` and `/track/mine` use, so the
    // word here must match theirs -- and the raw token must never appear.
    expect(screen.getByText('En route')).toBeInTheDocument();
    expect(screen.getByText('5m late')).toBeInTheDocument();
    expect(screen.queryByText('en_route')).not.toBeInTheDocument();
  });

  /** A member whose identity provider has no name on file for them used to
   * arrive here as a BLANK `displayName`/`addedByName`, not a null one
   * (Authentik and friends send `"name": ""` rather than omitting the
   * claim, and the backend stored/served exactly what it was sent).
   * `?? 'A member'` never fires for `''`, so both of these rendered as an
   * empty gap where the name should be -- the member row showed only a
   * role badge, and the attribution line read "Shared by " with nothing
   * after it.
   *
   * The backend now normalizes blanks to `null` on both write and read, so
   * these two guards are for rows written before it did (and for any other
   * future producer of a blank) -- worth keeping precisely because the
   * failure mode is silent: a blank label looks like a layout bug, not a
   * missing value. */
  it('falls back to a placeholder when a display name is blank rather than null', async () => {
    vi.mocked(getGroup).mockResolvedValue({
      id: 'grp-1',
      name: 'Family',
      ownerId: 'user-1',
      ownerName: 'Alex',
      ownerTag: null,
      memberCount: 2,
      role: 'member',
      inviteLink: null,
    });
    vi.mocked(getGroupMembers).mockResolvedValue([
      { userId: 'user-1', displayName: 'Alex', displayTag: null, role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
      { userId: 'user-2', displayName: '   ', displayTag: null, role: 'member', joinedAt: '2026-09-02T00:00:00Z' },
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
        status: null,
        delayMinutes: null,
        customName: null,
        addedBy: 'user-2',
        addedByName: '',
        addedByTag: null,
      },
    ]);
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));

    expect(screen.getByText('A member')).toBeInTheDocument();
    expect(screen.getByText(/Shared by a member/)).toBeInTheDocument();
  });

  /** The Entra-ID case. `preferred_username` there IS the user's
   * email-shaped UPN, so the backend declines to name ANY member of the
   * group (it never shows an address) and every row used to read as the
   * identical "A member" -- an admin looking at this list had no way to
   * tell which row was whom, or which of them shared the train below.
   * `displayTag` is what separates them, and the sharer's tag matches
   * their own row in the member list so the two can be read together. */
  it('tells placeholder-rendered members apart by their display tag', async () => {
    vi.mocked(getGroup).mockResolvedValue({
      id: 'grp-1',
      name: 'Family',
      ownerId: 'user-1',
      ownerName: null,
      ownerTag: 'a1b2c3',
      memberCount: 3,
      role: 'owner',
      inviteLink: null,
    });
    vi.mocked(getGroupMembers).mockResolvedValue([
      { userId: 'user-1', displayName: null, displayTag: 'a1b2c3', role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
      { userId: 'user-2', displayName: null, displayTag: 'd4e5f6', role: 'member', joinedAt: '2026-09-02T00:00:00Z' },
      { userId: 'user-3', displayName: 'Ada Rider', displayTag: null, role: 'member', joinedAt: '2026-09-03T00:00:00Z' },
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
        status: null,
        delayMinutes: null,
        customName: null,
        addedBy: 'user-2',
        addedByName: null,
        addedByTag: 'd4e5f6',
      },
    ]);
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: null });

    renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));

    expect(screen.getByText('A member (#a1b2c3)')).toBeInTheDocument();
    expect(screen.getByText('A member (#d4e5f6)')).toBeInTheDocument();
    // The member this app CAN name is untouched -- no suffix on a real name.
    expect(screen.getByText('Ada Rider')).toBeInTheDocument();
    // ...and the credit on the shared train points at the second member's
    // row rather than at an anonymous everyone.
    expect(screen.getByText(/Shared by a member \(#d4e5f6\)/)).toBeInTheDocument();
    expect(screen.queryByText('A member')).not.toBeInTheDocument();
  });

  it('never renders a Remove button for the owner row', async () => {
    vi.mocked(getGroup).mockResolvedValue({
      id: 'grp-1',
      name: 'Family',
      ownerId: 'user-1',
      ownerName: 'Alex',
      ownerTag: null,
      memberCount: 1,
      role: 'owner',
      inviteLink: null,
    });
    vi.mocked(getGroupMembers).mockResolvedValue([
      { userId: 'user-1', displayName: 'Alex', displayTag: null, role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
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
      displayTag: null,
      role: 'owner',
      joinedAt: '2026-09-01T00:00:00Z',
    };
    const ADMIN: GroupMember = {
      userId: 'user-admin',
      displayName: 'Adam',
      displayTag: null,
      role: 'admin',
      joinedAt: '2026-09-02T00:00:00Z',
    };
    const PLAIN: GroupMember = {
      userId: 'user-plain',
      displayName: 'Priya',
      displayTag: null,
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
        addedByTag: null,
      };
    }

    /** The rendered member row for `member`, so a per-row assertion can
     * say WHICH row a control sits on rather than just how many of it the
     * whole page has. `MemberRow` renders
     * `<Group>{<Group><Text>label</Text><Badge/></Group>}{controls}</Group>`,
     * so the row is two levels up from the label's own element. */
    function memberRow(member: GroupMember): HTMLElement {
      const label = screen.getByText(member.displayName as string);
      const row = label.parentElement?.parentElement;
      if (!row) throw new Error(`no rendered row found for ${member.displayName}`);
      return row;
    }

    /** Renders the page as `viewer`, with `viewerRole` as their role on the
     * group -- the two always agree in reality, so they're set together. */
    async function renderAs(viewer: GroupMember, viewerRole: GroupRole, trains: GroupTrain[] = []) {
      vi.mocked(getGroup).mockResolvedValue({
        id: 'grp-1',
        name: 'Family',
        ownerId: OWNER.userId,
        ownerName: OWNER.displayName,
        ownerTag: null,
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

    it('an owner viewer sees "Demote to member" on the admin row and nowhere else', async () => {
      await renderAs(OWNER, 'owner');
      // Asserted per ROW, not as a count: "exactly one demote button on
      // the page" would still pass if it were rendered against the
      // OWNER's own row instead -- the permanent-owner row the backend
      // 403s, i.e. precisely the "button whose only outcome is a 403"
      // this page's own doc comment exists to prevent.
      expect(within(memberRow(ADMIN)).getByRole('button', { name: 'Demote to member' })).toBeInTheDocument();
      expect(within(memberRow(OWNER)).queryByRole('button', { name: 'Demote to member' })).not.toBeInTheDocument();
      // A plain member has no admin role to lose; they get the opposite
      // control instead, and only that one.
      expect(within(memberRow(PLAIN)).queryByRole('button', { name: 'Demote to member' })).not.toBeInTheDocument();
      expect(within(memberRow(PLAIN)).getByRole('button', { name: 'Promote to admin' })).toBeInTheDocument();
      expect(within(memberRow(ADMIN)).queryByRole('button', { name: 'Promote to admin' })).not.toBeInTheDocument();
    });

    it('an admin viewer does NOT see "Demote to member" (the backend is owner-only)', async () => {
      await renderAs(ADMIN, 'admin');
      expect(screen.queryByRole('button', { name: 'Demote to member' })).not.toBeInTheDocument();
    });

    it('a plain member viewer does NOT see "Demote to member"', async () => {
      await renderAs(PLAIN, 'member');
      expect(screen.queryByRole('button', { name: 'Demote to member' })).not.toBeInTheDocument();
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

    // Task 1.5 (WCAG 2.5.3): the row's `Group wrap="nowrap"` (`StatusRow`)
    // pairs the train's display name with this button -- it must not be
    // crushable by a long name/subtitle.
    it('gives "Remove from group" a shrink guard', async () => {
      await renderAs(ADMIN, 'admin', [sharedTrain(PLAIN.userId, 44)]);
      expectShrinkGuarded(screen.getByRole('button', { name: 'Remove from group' }));
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
        ownerTag: null,
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

  describe('shared custom lines', () => {
    const SHARER = 'user-sharer';

    function line(overrides: Partial<GroupCustomLine> = {}): GroupCustomLine {
      return {
        lineId: 'custom-my-commute',
        lineName: 'My Commute',
        grantedBy: SHARER,
        grantedByName: 'Sam',
        grantedByTag: null,
        ...overrides,
      };
    }

    function report(): LineStatusReport {
      return {
        $type: 'DistantSignal.LineStatusReport',
        id: 'custom-my-commute',
        name: 'My Commute',
        modeName: 'national-rail',
        operators: ['SW'],
        computedAt: '2026-09-15T09:00:00Z',
        lineStatuses: [
          {
            statusSeverity: 6,
            statusSeverityDescription: 'Severe Delays',
            reason: 'signalling',
            sampleAvailability: { state: 'no-coverage' },
          } as never,
        ],
      };
    }

    async function renderAsViewer(viewerId: string, role: GroupRole) {
      vi.mocked(getGroup).mockResolvedValue({
        id: 'grp-1',
        name: 'Family',
        ownerId: 'user-owner',
        ownerName: 'Alex',
        ownerTag: null,
        memberCount: 3,
        role,
        inviteLink: null,
      });
      vi.mocked(getGroupMembers).mockResolvedValue([]);
      vi.mocked(getGroupTrains).mockResolvedValue([]);
      vi.mocked(getSession).mockResolvedValue({
        authenticated: true,
        id: viewerId,
        email: null,
        name: null,
      });
      renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));
    }

    it('renders an empty state when nothing has been shared', async () => {
      await renderAsViewer('user-owner', 'owner');
      expect(
        screen.getByText('No custom lines have been shared into this group yet.'),
      ).toBeInTheDocument();
    });

    it('renders a shared line with its attribution, status badge and link out', async () => {
      vi.mocked(getGroupCustomLines).mockResolvedValue([line()]);
      vi.mocked(getLineStatus).mockResolvedValue([report()]);
      await renderAsViewer('user-other', 'member');

      expect(screen.getByRole('heading', { name: 'Shared custom lines' })).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'My Commute' })).toHaveAttribute(
        'href',
        '/lines/custom-my-commute',
      );
      expect(screen.getByText('Shared by Sam')).toBeInTheDocument();
      expect(screen.getByText('Severe Delays')).toBeInTheDocument();
    });

    it('still renders the row when no status has been computed for the line yet', async () => {
      vi.mocked(getGroupCustomLines).mockResolvedValue([line()]);
      vi.mocked(getLineStatus).mockRejectedValue(new ApiNotFoundError('no matching line(s)'));
      await renderAsViewer('user-other', 'member');

      expect(screen.getByRole('link', { name: 'My Commute' })).toBeInTheDocument();
    });

    it('a plain member who did not share it sees no "Stop sharing" control', async () => {
      vi.mocked(getGroupCustomLines).mockResolvedValue([line()]);
      await renderAsViewer('user-bystander', 'member');

      expect(screen.queryByRole('button', { name: 'Stop sharing' })).not.toBeInTheDocument();
    });

    it('the member who shared it DOES see "Stop sharing", even as a plain member', async () => {
      vi.mocked(getGroupCustomLines).mockResolvedValue([line()]);
      await renderAsViewer(SHARER, 'member');

      expect(screen.getByRole('button', { name: 'Stop sharing' })).toBeInTheDocument();
    });

    it('an admin sees "Stop sharing" on someone else\'s shared line', async () => {
      vi.mocked(getGroupCustomLines).mockResolvedValue([line()]);
      await renderAsViewer('user-admin', 'admin');

      expect(screen.getByRole('button', { name: 'Stop sharing' })).toBeInTheDocument();
    });

    // Task 1.5 (WCAG 2.5.3): the row pairs a (potentially long) line name
    // with this status badge and "Stop sharing" button in a `Group
    // wrap="nowrap"` (`StatusRow`) -- without a shrink guard on both,
    // either could be crushed to nothing instead of the title truncating.
    it('gives the status badge and the "Stop sharing" button a shrink guard against a long line name', async () => {
      vi.mocked(getGroupCustomLines).mockResolvedValue([
        line({ lineName: 'An Implausibly Long Custom Line Name Chosen To Threaten The Row Layout' }),
      ]);
      vi.mocked(getLineStatus).mockResolvedValue([report()]);
      await renderAsViewer(SHARER, 'member');

      expectShrinkGuarded(screen.getByText('Severe Delays'));
      expectShrinkGuarded(screen.getByRole('button', { name: 'Stop sharing' }));
    });

    it('never offers an Edit or Delete control for a line the viewer does not own', async () => {
      // A grant is read-only: not even a group owner can edit or delete a
      // member's custom line, and this page must never suggest otherwise.
      vi.mocked(getGroupCustomLines).mockResolvedValue([line()]);
      await renderAsViewer('user-owner', 'owner');

      expect(screen.queryByRole('button', { name: /^Edit/ })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /Delete line/i })).not.toBeInTheDocument();
    });
  });
});
