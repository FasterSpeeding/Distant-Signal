import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DeleteJourneyButton } from './DeleteJourneyButton';

const pushMock = vi.fn();
const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, refresh: refreshMock }),
  usePathname: () => '/journeys/167',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('DeleteJourneyButton', () => {
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
    renderWithMantine(<DeleteJourneyButton journeyId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete journey' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('warns that every leg is removed and this cannot be undone', async () => {
    renderWithMantine(<DeleteJourneyButton journeyId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete journey' }));
    await waitFor(() => expect(screen.getByText(/removes every leg on this journey/)).toBeInTheDocument());
    expect(screen.getByText(/cannot be undone/)).toBeInTheDocument();
  });

  it('DELETEs the journey and redirects to /track/mine on success', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<DeleteJourneyButton journeyId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete journey' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete journey' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete journey' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/167', { method: 'DELETE' });
    });
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/track/mine'));
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('shows an error and does not navigate on a failed delete', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no journey with that id', { status: 404 }));

    renderWithMantine(<DeleteJourneyButton journeyId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete journey' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete journey' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete journey' }));

    await waitFor(() => {
      expect(screen.getByText('no journey with that id')).toBeInTheDocument();
    });
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('a 401 shows a login prompt instead of the raw backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<DeleteJourneyButton journeyId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete journey' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete journey' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete journey' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to delete this journey' });
    expect(loginLink).toBeInTheDocument();
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('cancelling the modal does not call DELETE', async () => {
    const fetchMock = vi.mocked(fetch);
    renderWithMantine(<DeleteJourneyButton journeyId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete journey' }));
    await waitFor(() => screen.getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
