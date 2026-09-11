import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AddToGroupButton } from './AddToGroupButton';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/train/by-id/42',
  useSearchParams: () => new URLSearchParams(''),
}));

function groupsResponse(
  groups: { id: string; name: string; role: string; memberCount: number }[] = [
    { id: 'grp-1', name: 'Commuters', role: 'member', memberCount: 3 },
    { id: 'grp-2', name: 'Family', role: 'owner', memberCount: 2 },
  ],
) {
  return new Response(JSON.stringify(groups), { status: 200 });
}

describe('AddToGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders nothing when the viewer is in zero groups', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(groupsResponse([]));

    renderWithMantine(<AddToGroupButton trainSubscriptionId={7} />);

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/groups'));
    // Let the resulting state update flush, then confirm no button/modal
    // content ever appears -- no button, no empty state, per this
    // component's own "don't clutter the common case" contract.
    await waitFor(() => expect(screen.queryByRole('button')).not.toBeInTheDocument());
  });

  it('renders nothing while the groups fetch fails', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockRejectedValue(new Error('network blip'));

    renderWithMantine(<AddToGroupButton trainSubscriptionId={7} />);

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/groups'));
    await waitFor(() => expect(screen.queryByRole('button')).not.toBeInTheDocument());
  });

  it('shows the button and a group picker once the viewer has at least one group', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(groupsResponse());

    renderWithMantine(<AddToGroupButton trainSubscriptionId={7} />);

    const button = await screen.findByRole('button', { name: 'Add to group' });
    fireEvent.click(button);

    const [select] = await screen.findAllByLabelText('Group');
    fireEvent.click(select);
    expect(await screen.findByText('Commuters')).toBeInTheDocument();
    expect(screen.getByText('Family')).toBeInTheDocument();
  });

  it('POSTs the chosen groupId and shows a success confirmation without closing the modal', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/groups') return Promise.resolve(groupsResponse());
      return Promise.resolve(new Response(null, { status: 204 }));
    });

    renderWithMantine(<AddToGroupButton trainSubscriptionId={7} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Add to group' }));
    const [select] = await screen.findAllByLabelText('Group');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText('Commuters'));
    fireEvent.click(screen.getByRole('button', { name: 'Share' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/groups/grp-1/trains',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trainSubscriptionId: 7 }) }),
      );
    });
    expect(await screen.findByText('Added to Commuters.')).toBeInTheDocument();
    // The modal itself is still open -- the confirm button is still there,
    // allowing a second group to be picked without reopening.
    expect(screen.getByRole('button', { name: 'Share' })).toBeInTheDocument();
  });

  it('shows a real error, not silence, when the share request fails', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/groups') return Promise.resolve(groupsResponse());
      return Promise.resolve(new Response('Something went wrong', { status: 500 }));
    });

    renderWithMantine(<AddToGroupButton trainSubscriptionId={7} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Add to group' }));
    const [select] = await screen.findAllByLabelText('Group');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText('Commuters'));
    fireEvent.click(screen.getByRole('button', { name: 'Share' }));

    expect(await screen.findByText('Something went wrong')).toBeInTheDocument();
    expect(screen.queryByText(/Added to/)).not.toBeInTheDocument();
  });

  it('shows a login prompt on a 401 rather than the raw rejection text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/groups') return Promise.resolve(groupsResponse());
      return Promise.resolve(new Response('unauthorized', { status: 401 }));
    });

    renderWithMantine(<AddToGroupButton trainSubscriptionId={7} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Add to group' }));
    const [select] = await screen.findAllByLabelText('Group');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText('Commuters'));
    fireEvent.click(screen.getByRole('button', { name: 'Share' }));

    expect(await screen.findByText('Log in to share this train')).toBeInTheDocument();
    expect(screen.queryByText('unauthorized')).not.toBeInTheDocument();
  });
});
