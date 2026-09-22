import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AddJourneyLegButton } from './AddJourneyLegButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/journeys/1',
  useSearchParams: () => new URLSearchParams(''),
}));

function legResponse() {
  return new Response(JSON.stringify({ legId: 9, trackingId: null }), { status: 200 });
}

describe('AddJourneyLegButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('pre-fills the origin field from priorDestinationCrs on open', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const origin = await screen.findByLabelText('Origin CRS');
    expect(origin).toHaveValue('WAT');
  });

  it('POSTs a window-mode request with the entered origin/destination/date', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(legResponse());

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByLabelText('Destination CRS');
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    const serviceDate = screen.getByLabelText('Service date');
    fireEvent.change(serviceDate, { target: { value: '2026-09-22' } });

    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Journeys/1/legs',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({
            mode: 'window',
            originCrs: 'WAT',
            destinationCrs: 'CLJ',
            serviceDate: '2026-09-22',
          }),
        }),
      );
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('POSTs a knownTrain-mode request when the direct-pick mode is selected', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(legResponse());

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    fireEvent.click(await screen.findByText('I know the train'));
    const trainUid = await screen.findByLabelText('Train UID');
    fireEvent.change(trainUid, { target: { value: 'A12345' } });
    const serviceDate = screen.getByLabelText('Service date');
    fireEvent.change(serviceDate, { target: { value: '2026-09-22' } });

    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Journeys/1/legs',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ mode: 'knownTrain', trainUid: 'A12345', serviceDate: '2026-09-22' }),
        }),
      );
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('surfaces the server error text on a non-OK response', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no schedule matched that window', { status: 400 }));

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByLabelText('Destination CRS');
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    const serviceDate = screen.getByLabelText('Service date');
    fireEvent.change(serviceDate, { target: { value: '2026-09-22' } });

    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    expect(await screen.findByText('no schedule matched that window')).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
