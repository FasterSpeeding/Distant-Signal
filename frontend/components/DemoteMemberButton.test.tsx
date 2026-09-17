import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DemoteMemberButton } from './DemoteMemberButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('DemoteMemberButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('does not POST anything until the confirm step', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ userId: 'user-2', role: 'member' }), { status: 200 }));

    renderWithMantine(<DemoteMemberButton groupId="grp-1" userId="user-2" name="Adam" />);
    fireEvent.click(screen.getByRole('button', { name: 'Demote to member' }));

    await waitFor(() => screen.getByRole('button', { name: 'Confirm demote member' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('POSTs the demote request and refreshes on confirm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ userId: 'user-2', role: 'member' }), { status: 200 }));

    renderWithMantine(<DemoteMemberButton groupId="grp-1" userId="user-2" name="Adam" />);
    fireEvent.click(screen.getByRole('button', { name: 'Demote to member' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm demote member' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm demote member' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/members/user-2/demote', { method: 'POST' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('cancelling closes the modal without demoting anyone', async () => {
    const fetchMock = vi.mocked(fetch);

    renderWithMantine(<DemoteMemberButton groupId="grp-1" userId="user-2" name="Adam" />);
    fireEvent.click(screen.getByRole('button', { name: 'Demote to member' }));
    await waitFor(() => screen.getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(fetchMock).not.toHaveBeenCalled();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  /** The 403 an admin gets if they reach this endpoint anyway (the page
   * never renders this control for them, but the backend is the
   * authority) -- and the same shape the owner sees for their own row. */
  it('a 403 shows the backend error text and does not refresh', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response("the group owner can't be demoted", { status: 403 }));

    renderWithMantine(<DemoteMemberButton groupId="grp-1" userId="user-2" name="Adam" />);
    fireEvent.click(screen.getByRole('button', { name: 'Demote to member' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm demote member' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm demote member' }));

    await waitFor(() => {
      expect(screen.getByText("the group owner can't be demoted")).toBeInTheDocument();
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('a 409 (already a plain member) shows the backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response("that member isn't an admin", { status: 409 }));

    renderWithMantine(<DemoteMemberButton groupId="grp-1" userId="user-2" name="Adam" />);
    fireEvent.click(screen.getByRole('button', { name: 'Demote to member' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm demote member' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm demote member' }));

    await waitFor(() => {
      expect(screen.getByText("that member isn't an admin")).toBeInTheDocument();
    });
  });

  it('a 401 offers a login link rather than a raw error', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('unauthorized', { status: 401 }));

    renderWithMantine(<DemoteMemberButton groupId="grp-1" userId="user-2" name="Adam" />);
    fireEvent.click(screen.getByRole('button', { name: 'Demote to member' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm demote member' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm demote member' }));

    await waitFor(() => {
      expect(screen.getByText('Log in to demote this member')).toBeInTheDocument();
    });
    expect(screen.queryByText('unauthorized')).not.toBeInTheDocument();
  });
});
