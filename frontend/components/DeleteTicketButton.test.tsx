import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DeleteTicketButton } from './DeleteTicketButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/track/mine',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('DeleteTicketButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('does not call DELETE until the confirmation modal is confirmed', () => {
    const fetchMock = vi.mocked(fetch);
    renderWithMantine(<DeleteTicketButton ticketId={5} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('DELETEs the ticket and refreshes the page on confirm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<DeleteTicketButton ticketId={5} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/tickets/5', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('shows an error and does not refresh on a failed delete', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no ticket with that id', { status: 404 }));

    renderWithMantine(<DeleteTicketButton ticketId={5} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(screen.getByText('no ticket with that id')).toBeInTheDocument();
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });

  // This button only ever renders for a ticket the enclosing Server
  // Component just fetched, so a 401 here can only come from a session
  // that lapses between page load and this click -- same narrow race
  // DeleteTrainButton already reasons about for its own delete.
  it('a 401 shows a login prompt instead of the raw backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<DeleteTicketButton ticketId={5} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to delete this ticket' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login?return_to=%2Ftrack%2Fmine');
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
