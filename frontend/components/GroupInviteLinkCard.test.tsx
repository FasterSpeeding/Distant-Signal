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

  /** The origin is read in a mount effect, never in the render body --
   * this component is rendered by an async Server Component, so a
   * render-body `window.location.origin` threw a `ReferenceError` during
   * SSR for every admin/owner. jsdom always provides `window`, so this
   * suite structurally cannot reproduce the SSR crash itself (the real
   * guard for that is `npm run build` exercising the `/groups/[id]`
   * route); what it CAN pin down is that the effect actually runs and
   * produces the absolute URL, i.e. the deferred read still works. */
  it('builds an absolute URL from the origin once mounted', async () => {
    renderWithMantine(
      <GroupInviteLinkCard groupId="grp-1" inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }} />,
    );
    await waitFor(() =>
      expect(screen.getByDisplayValue(`${window.location.origin}/groups/join/tok123`)).toBeInTheDocument(),
    );
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

  it('a 401 on Regenerate shows a login prompt, not the generic error', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Regenerate' }));

    expect(await screen.findByRole('link', { name: 'Log in to manage this invite link' })).toBeInTheDocument();
    expect(screen.queryByText('Could not create a new invite link.')).not.toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('a 401 on Revoke shows a login prompt, not the generic error', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(
      <GroupInviteLinkCard groupId="grp-1" inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Revoke' }));

    expect(await screen.findByRole('link', { name: 'Log in to manage this invite link' })).toBeInTheDocument();
    expect(screen.queryByText('Could not revoke the invite link.')).not.toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('a non-401 failure still shows the generic error', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('boom', { status: 500 }));

    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Regenerate' }));

    expect(await screen.findByText('Could not create a new invite link.')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Log in to manage this invite link' })).not.toBeInTheDocument();
  });
});
