import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { expectShrinkGuarded } from '@/test/shrinkGuard';
import GroupDetailPage from './page';
import {
  getGroup,
  getGroupCustomLines,
  getGroupJourneys,
  getGroupMembers,
  getGroupTrains,
  getLineStatus,
  getSession,
  ApiNotFoundError,
  ApiUnauthorizedError,
} from '@/lib/api';
import type {
  GroupCustomLine,
  GroupJourney,
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
    getGroupJourneys: vi.fn(),
    getGroupMembers: vi.fn(),
    getGroupTrains: vi.fn(),
    getLineStatus: vi.fn(),
    getSession: vi.fn(),
  };
});

// Every test that gets past the group fetch renders the shared-custom-lines
// and shared-journeys sections, so their defaults need to be set here too;
// individual tests below override what they care about.
beforeEach(() => {
  vi.mocked(getGroupCustomLines).mockResolvedValue([]);
  vi.mocked(getLineStatus).mockResolvedValue([]);
  vi.mocked(getGroupJourneys).mockResolvedValue([]);
});

vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn(), push: vi.fn() }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

// `getSiteOrigin()` (lib/siteOrigin.ts), called unconditionally by this
// page to build GroupInviteLinkCard's `origin` prop, reads `next/headers`
// when `NEXT_PUBLIC_SITE_URL` isn't set -- there is no Next request
// context in a unit test. Same stub shape lib/api.test.ts's own
// `next/headers` mock uses, extended with the `.get()` `getSiteOrigin`
// needs.
vi.mock('next/headers', () => ({
  headers: async () => ({ get: () => null }),
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

  // Review §3.2.5: the only way back to `/groups` used to be the browser's
  // own Back button, which fails outright for a visitor who followed a
  // deep link straight to a group's detail page.
  it('links back to /groups above the page heading', async () => {
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
    expect(screen.getByRole('link', { name: '← Groups' })).toHaveAttribute('href', '/groups');
  });

  // Review §3.2.5: nothing marked which row in the member list was the
  // current viewer's own -- awkward to spot even with a real name, and
  // outright ambiguous for a placeholder-rendered member matching their
  // own opaque tag against the list.
  it('marks the current user\'s own row with "(you)" and no other row', async () => {
    vi.mocked(getGroup).mockResolvedValue({
      id: 'grp-1',
      name: 'Family',
      ownerId: 'user-1',
      ownerName: 'Alex',
      ownerTag: null,
      memberCount: 2,
      role: 'owner',
      inviteLink: null,
    });
    vi.mocked(getGroupMembers).mockResolvedValue([
      { userId: 'user-1', displayName: 'Alex', displayTag: null, role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
      { userId: 'user-2', displayName: 'Sam', displayTag: null, role: 'member', joinedAt: '2026-09-02T00:00:00Z' },
    ]);
    vi.mocked(getGroupTrains).mockResolvedValue([]);
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    renderWithMantine(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));
    expect(screen.getAllByText('(you)')).toHaveLength(1);
    expect(screen.getByText('Alex').parentElement).toContainElement(screen.getByText('(you)'));
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

    // Review §3.2.1: the owner leaving a group that survives (other members
    // remain) always transfers ownership -- `remove_member`'s own successor
    // query picks the longest-standing remaining ADMIN over a longer-standing
    // plain member, so the copy here must name Adam, not Priya, even though
    // Priya joined first.
    it("names the longest-standing admin as the successor when the owner leaves", async () => {
      await renderAs(OWNER, 'owner');

      fireEvent.click(screen.getByRole('button', { name: 'Leave group' }));
      await waitFor(() => screen.getByText(/Adam will become the new owner/));
    });

    it('names nobody as successor when the sole owner leaves (the group is deleted instead)', async () => {
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
      expect(screen.queryByText(/will become the new owner/)).not.toBeInTheDocument();
    });

    it('names no successor for a non-owner leaving', async () => {
      await renderAs(PLAIN, 'member');

      fireEvent.click(screen.getByRole('button', { name: 'Leave group' }));
      await waitFor(() => screen.getByText(/lose access to every train shared/));
      expect(screen.queryByText(/will become the new owner/)).not.toBeInTheDocument();
    });

    // Review §3.2.1: "Delete group" moved out of the header into its own
    // "Danger zone" section at the foot of the page.
    it('renders "Delete group" inside a "Danger zone" heading for an owner', async () => {
      await renderAs(OWNER, 'owner');
      expect(screen.getByRole('heading', { name: 'Danger zone' })).toBeInTheDocument();
    });

    it('renders no "Danger zone" section for a non-owner', async () => {
      await renderAs(ADMIN, 'admin');
      expect(screen.queryByRole('heading', { name: 'Danger zone' })).not.toBeInTheDocument();
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

  // Item 7 of the 2026-09-22 UX review's fix-cycle follow-up: no seed data
  // ever created a group with an actual shared journey in it, so
  // `SharedJourneyRow` (page.tsx) had only ever been reviewed in its EMPTY
  // state -- every test above this point that touches journeys at all
  // leaves `getGroupJourneys` at its `beforeEach` default of `[]`. Live
  // visual verification (seeding a real group/journey/share against the
  // running preview and looking at the rendered page) was attempted but
  // not possible this session: the preview's own API process (port 8080)
  // was reachable earlier in this session but had gone down by the time
  // this item was reached, and standing up a private replacement against
  // the same shared database was judged too risky for a check-only task.
  // This describe block is the fallback the task's own instructions name
  // for that case: a realistic component-level fixture, checked for the
  // same class of issues (truncation, missing labels, colour-only
  // signals) the rest of this review cycle found elsewhere. Mirrors
  // 'shared custom lines' above field-for-field.
  describe('shared journeys', () => {
    const SHARER = 'user-sharer';

    function journey(overrides: Partial<GroupJourney> = {}): GroupJourney {
      return {
        journeyId: 501,
        customName: null,
        legCount: 1,
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
        addedBy: SHARER,
        addedByName: 'Sam',
        addedByTag: null,
        ...overrides,
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
        screen.getByText('No journeys have been shared into this group yet.'),
      ).toBeInTheDocument();
    });

    it('renders a shared journey with its route, attribution, status badge and link out', async () => {
      vi.mocked(getGroupJourneys).mockResolvedValue([journey()]);
      await renderAsViewer('user-other', 'member');

      expect(screen.getByRole('heading', { name: 'Shared journeys' })).toBeInTheDocument();
      expect(
        screen.getByRole('link', { name: /Woking \(WOK\) → London Waterloo \(WAT\)/ }),
      ).toHaveAttribute('href', '/journeys/501');
      expect(screen.getByText('Shared by Sam')).toBeInTheDocument();
      // Same shared `TrackedTrainStatusBadge` `/` and `/track/mine` use --
      // the raw `status` enum token must never leak into the page.
      expect(screen.getByText('En route')).toBeInTheDocument();
      expect(screen.getByText('5m late')).toBeInTheDocument();
      expect(screen.queryByText('en_route')).not.toBeInTheDocument();
    });

    it('appends a "+N more legs" suffix for a multi-leg journey, and omits it for a single leg', async () => {
      vi.mocked(getGroupJourneys).mockResolvedValue([
        journey({ journeyId: 501, legCount: 1 }),
        journey({ journeyId: 502, legCount: 3, customName: 'Scotland trip' }),
      ]);
      await renderAsViewer('user-other', 'member');

      expect(screen.getByText(/Woking \(WOK\) → London Waterloo \(WAT\)/)).toBeInTheDocument();
      expect(screen.queryByText(/\+0 more leg/)).not.toBeInTheDocument();
      expect(screen.getByText(/Scotland trip \(\+2 more legs\)/)).toBeInTheDocument();
    });

    it('shows a red "Unmatched" badge, not the raw resolutionStatus, for a leg with no train bound yet', async () => {
      // `resolutionStatus` is nullable on the wire (a journey's first leg
      // may have no bound train yet) -- SharedJourneyRow coalesces this to
      // 'unresolved' before handing it to TrackedTrainStatusBadge, whose
      // 'unresolved' branch is the one WCAG 1.4.1 colour-plus-text case
      // (red AND the word "Unmatched", never colour alone).
      vi.mocked(getGroupJourneys).mockResolvedValue([
        journey({ resolutionStatus: null, status: null, delayMinutes: null, trainUid: null }),
      ]);
      await renderAsViewer('user-other', 'member');

      expect(screen.getByText('Unmatched')).toBeInTheDocument();
      expect(screen.queryByText('null')).not.toBeInTheDocument();
    });

    // Same blank-vs-null attribution gap `GroupTrain.addedByName` already
    // has a regression test for above -- a blank string used to render as
    // "Shared by " with nothing after it rather than falling back to the
    // generic placeholder.
    it('falls back to a placeholder when the sharer\'s display name is blank rather than null', async () => {
      vi.mocked(getGroupJourneys).mockResolvedValue([journey({ addedByName: '' })]);
      await renderAsViewer('user-other', 'member');

      expect(screen.getByText(/Shared by a member/)).toBeInTheDocument();
    });

    it('a plain member who did not share it sees no "Remove from group" control', async () => {
      vi.mocked(getGroupJourneys).mockResolvedValue([journey()]);
      await renderAsViewer('user-bystander', 'member');

      expect(screen.queryByRole('button', { name: 'Remove from group' })).not.toBeInTheDocument();
    });

    it('the member who shared it DOES see "Remove from group", even as a plain member', async () => {
      vi.mocked(getGroupJourneys).mockResolvedValue([journey()]);
      await renderAsViewer(SHARER, 'member');

      expect(screen.getByRole('button', { name: 'Remove from group' })).toBeInTheDocument();
    });

    it('an admin sees "Remove from group" on someone else\'s shared journey', async () => {
      vi.mocked(getGroupJourneys).mockResolvedValue([journey()]);
      await renderAsViewer('user-admin', 'admin');

      expect(screen.getByRole('button', { name: 'Remove from group' })).toBeInTheDocument();
    });

    // Task 1.5 (WCAG 2.5.3), same shrink-guard convention
    // 'shared custom lines' already applies above: the row pairs a
    // (potentially long) route/custom name with the status badge and
    // "Remove from group" button in a `Group wrap="nowrap"` (`StatusRow`)
    // -- without a shrink guard on both, either could be crushed to
    // nothing instead of the title truncating.
    it('gives the status badge and the "Remove from group" button a shrink guard against a long custom name', async () => {
      vi.mocked(getGroupJourneys).mockResolvedValue([
        journey({
          customName: 'An Implausibly Long Journey Name Chosen To Threaten The Row Layout End To End',
        }),
      ]);
      await renderAsViewer(SHARER, 'member');

      expectShrinkGuarded(screen.getByText('En route'));
      expectShrinkGuarded(screen.getByRole('button', { name: 'Remove from group' }));
    });
  });
});
