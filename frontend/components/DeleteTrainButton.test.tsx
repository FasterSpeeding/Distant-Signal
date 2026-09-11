import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DeleteTrainButton } from './DeleteTrainButton';

const pushMock = vi.fn();
const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, refresh: refreshMock }),
  usePathname: () => '/train/by-id/42',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('DeleteTrainButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('does not call DELETE until the confirmation modal is confirmed', () => {
    const fetchMock = vi.mocked(fetch);
    renderWithMantine(<DeleteTrainButton trackingId={42} sharedGroupCount={0} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  // Default `afterDelete` ('redirect') -- the by-id page's own behaviour,
  // unchanged by the new prop.
  it('DELETEs the tracked train and redirects to /track/mine on confirm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<DeleteTrainButton trackingId={42} sharedGroupCount={0} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/42', { method: 'DELETE' });
    });
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/track/mine'));
    expect(refreshMock).not.toHaveBeenCalled();
  });

  // `afterDelete="refresh"` -- `/train/[uid]/[date]`'s own behaviour: stay
  // put and let the Server Component re-fetch instead of navigating away.
  it('DELETEs the tracked train and calls router.refresh() instead of redirecting when afterDelete is "refresh"', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<DeleteTrainButton trackingId={42} sharedGroupCount={0} afterDelete="refresh" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/42', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('shows an error and does not redirect on a failed delete', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no tracked train with that id', { status: 404 }));

    renderWithMantine(<DeleteTrainButton trackingId={42} sharedGroupCount={0} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(screen.getByText('no tracked train with that id')).toBeInTheDocument();
    });
    expect(pushMock).not.toHaveBeenCalled();
  });

  // Both train detail pages only ever render this button once they already
  // have the tracked train's state in hand, so a 401 here can only come
  // from a session that lapses between page load and this click. Same
  // `needsLogin` treatment as `DeleteLineButton`/`PinToggle`: a login
  // prompt, never the raw backend rejection text.
  it('a 401 shows a login prompt instead of the raw backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<DeleteTrainButton trackingId={42} sharedGroupCount={0} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to delete this tracked train' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login?return_to=%2Ftrain%2Fby-id%2F42');
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  // sharedGroupCount === 0: today's exact modal copy, no extra warning line.
  it('shows no group-sharing warning when the train is not shared into any group', async () => {
    renderWithMantine(<DeleteTrainButton trackingId={42} sharedGroupCount={0} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    expect(screen.getByText('This cannot be undone.')).toBeInTheDocument();
    expect(screen.queryByText(/shared in/)).not.toBeInTheDocument();
  });

  // sharedGroupCount === 1: singular wording.
  it('warns about a single shared group, singular', async () => {
    renderWithMantine(<DeleteTrainButton trackingId={42} sharedGroupCount={1} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    expect(
      screen.getByText('This train is shared in 1 group — deleting it will remove it from that group too.'),
    ).toBeInTheDocument();
  });

  // sharedGroupCount > 1: plural wording.
  it('warns about multiple shared groups, plural', async () => {
    renderWithMantine(<DeleteTrainButton trackingId={42} sharedGroupCount={3} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    expect(
      screen.getByText('This train is shared in 3 groups — deleting it will remove it from those groups too.'),
    ).toBeInTheDocument();
  });
});
