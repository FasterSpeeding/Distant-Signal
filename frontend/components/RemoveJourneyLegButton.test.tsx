import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RemoveJourneyLegButton } from './RemoveJourneyLegButton';

const pushMock = vi.fn();
const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, refresh: refreshMock }),
  usePathname: () => '/journeys/167',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('RemoveJourneyLegButton', () => {
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
    renderWithMantine(<RemoveJourneyLegButton journeyId={167} legId={1} isOnlyLeg={false} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove leg' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('DELETEs the leg and calls router.refresh() when a sibling leg remains', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<RemoveJourneyLegButton journeyId={167} legId={1} isOnlyLeg={false} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove leg' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove leg' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove leg' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/167/legs/1', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('DELETEs the leg and redirects to /track/mine when it was the journey\'s only leg', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<RemoveJourneyLegButton journeyId={168} legId={2} isOnlyLeg={true} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove leg' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove leg' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove leg' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/168/legs/2', { method: 'DELETE' });
    });
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/track/mine'));
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('shows an error and does not navigate on a failed delete', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no journey leg with that id', { status: 404 }));

    renderWithMantine(<RemoveJourneyLegButton journeyId={167} legId={1} isOnlyLeg={false} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove leg' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove leg' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove leg' }));

    await waitFor(() => {
      expect(screen.getByText('no journey leg with that id')).toBeInTheDocument();
    });
    expect(pushMock).not.toHaveBeenCalled();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('a 401 shows a login prompt instead of the raw backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<RemoveJourneyLegButton journeyId={167} legId={1} isOnlyLeg={false} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove leg' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove leg' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove leg' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to remove this leg' });
    expect(loginLink).toBeInTheDocument();
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('warns that the whole journey goes too when this is the only leg', async () => {
    renderWithMantine(<RemoveJourneyLegButton journeyId={168} legId={2} isOnlyLeg={true} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove leg' }));
    await waitFor(() =>
      expect(
        screen.getByText(/removing it deletes the whole journey/),
      ).toBeInTheDocument(),
    );
  });

  it('does not warn about the whole journey when a sibling leg remains', async () => {
    renderWithMantine(<RemoveJourneyLegButton journeyId={167} legId={1} isOnlyLeg={false} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove leg' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove leg' }));
    expect(screen.queryByText(/deletes the whole journey/)).not.toBeInTheDocument();
  });
});
