import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { GroupInviteLinkCard } from './GroupInviteLinkCard';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('GroupInviteLinkCard', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows "No active invite link" when there is none', () => {
    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={null} />);
    expect(screen.getByText('No active invite link.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Revoke' })).not.toBeInTheDocument();
  });

  it('renders the full join URL built from the token', () => {
    renderWithMantine(
      <GroupInviteLinkCard groupId="grp-1" inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }} />,
    );
    expect(screen.getByDisplayValue(/\/groups\/join\/tok123$/)).toBeInTheDocument();
  });

  it('Regenerate POSTs and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ token: 'new', expiresAt: '2026-09-19T00:00:00Z' }), { status: 200 }));

    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }} />);
    fireEvent.click(screen.getByRole('button', { name: 'Regenerate' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/invite-link', { method: 'POST' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('Revoke DELETEs and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }} />);
    fireEvent.click(screen.getByRole('button', { name: 'Revoke' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/invite-link', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });
});
