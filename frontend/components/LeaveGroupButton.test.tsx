import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LeaveGroupButton } from './LeaveGroupButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('LeaveGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('DELETEs the current user as a member and navigates to /groups', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<LeaveGroupButton groupId="grp-1" currentUserId="user-1" />);
    fireEvent.click(screen.getByRole('button', { name: 'Leave group' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm leave group' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm leave group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/members/user-1', { method: 'DELETE' });
    });
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/groups'));
  });
});
