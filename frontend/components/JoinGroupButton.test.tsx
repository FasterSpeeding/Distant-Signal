import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JoinGroupButton } from './JoinGroupButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/groups/join/tok123',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('JoinGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('POSTs the join and navigates to the group', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ groupId: 'grp-1' }), { status: 200 }));

    renderWithMantine(<JoinGroupButton token="tok123" groupId="grp-1" />);
    fireEvent.click(screen.getByRole('button', { name: 'Join group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/join/tok123', { method: 'POST' });
    });
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/groups/grp-1'));
  });

  it('a 401 shows a login prompt instead of joining', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<JoinGroupButton token="tok123" groupId="grp-1" />);
    fireEvent.click(screen.getByRole('button', { name: 'Join group' }));

    await screen.findByRole('link', { name: 'Log in to join this group' });
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('an expired/revoked token (404) shows the backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('this invite link is invalid or has expired', { status: 404 }));

    renderWithMantine(<JoinGroupButton token="tok123" groupId="grp-1" />);
    fireEvent.click(screen.getByRole('button', { name: 'Join group' }));

    await waitFor(() => {
      expect(screen.getByText('this invite link is invalid or has expired')).toBeInTheDocument();
    });
    expect(pushMock).not.toHaveBeenCalled();
  });
});
