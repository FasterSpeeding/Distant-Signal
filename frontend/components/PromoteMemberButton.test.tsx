import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { PromoteMemberButton } from './PromoteMemberButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('PromoteMemberButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('confirms first, like DemoteMemberButton: no request until the dialog is confirmed', async () => {
    const fetchMock = vi.mocked(fetch);
    renderWithMantine(<PromoteMemberButton groupId="grp-1" userId="user-2" name="Alex" />);
    fireEvent.click(screen.getByRole('button', { name: 'Promote to admin' }));
    expect(await screen.findByText('Promote Alex to admin?')).toBeInTheDocument();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('POSTs the promote request on confirm, then refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ userId: 'user-2', role: 'admin' }), { status: 200 }));

    renderWithMantine(<PromoteMemberButton groupId="grp-1" userId="user-2" name="Alex" />);
    fireEvent.click(screen.getByRole('button', { name: 'Promote to admin' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Confirm promote member' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/members/user-2/promote', { method: 'POST' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('a 409 shows the backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('that member is already an admin or the owner', { status: 409 }));

    renderWithMantine(<PromoteMemberButton groupId="grp-1" userId="user-2" name="Alex" />);
    fireEvent.click(screen.getByRole('button', { name: 'Promote to admin' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Confirm promote member' }));

    await waitFor(() => {
      expect(screen.getByText('That member is already an admin or the owner.')).toBeInTheDocument();
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
