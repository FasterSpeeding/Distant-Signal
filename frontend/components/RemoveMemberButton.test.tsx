import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RemoveMemberButton } from './RemoveMemberButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('RemoveMemberButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('DELETEs the member and refreshes on confirm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<RemoveMemberButton groupId="grp-1" userId="user-2" name="Alex" />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove member' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove member' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/members/user-2', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('a 403 shows the backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response("the group owner can't be removed", { status: 403 }));

    renderWithMantine(<RemoveMemberButton groupId="grp-1" userId="user-2" name="Alex" />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove member' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove member' }));

    await waitFor(() => {
      expect(screen.getByText("the group owner can't be removed")).toBeInTheDocument();
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
