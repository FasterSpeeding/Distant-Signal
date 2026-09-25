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

  it('re-seeds the origin field from priorDestinationCrs on every open, not just the first', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const origin = await screen.findByLabelText('Origin CRS');
    fireEvent.change(origin, { target: { value: 'CLJ' } });
    expect(origin).toHaveValue('CLJ');

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => expect(screen.queryByLabelText('Origin CRS')).not.toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));
    const reopenedOrigin = await screen.findByLabelText('Origin CRS');
    expect(reopenedOrigin).toHaveValue('WAT');
  });

  it('POSTs a window-mode request with the entered origin/destination/date/departWindow', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(legResponse());

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByLabelText('Destination CRS');
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    const serviceDate = screen.getByLabelText('Service date');
    fireEvent.change(serviceDate, { target: { value: '2026-09-22' } });
    const departFrom = screen.getByLabelText('Earliest departure (optional)');
    fireEvent.change(departFrom, { target: { value: '09:00' } });

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
            departWindow: { after: '09:00', before: null },
            arriveWindow: { after: null, before: null },
          }),
        }),
      );
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('disables submit in window mode until at least one time bound is set', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByLabelText('Destination CRS');
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    const serviceDate = screen.getByLabelText('Service date');
    fireEvent.change(serviceDate, { target: { value: '2026-09-22' } });

    expect(screen.getByRole('button', { name: 'Add leg' })).toBeDisabled();

    const arriveTo = screen.getByLabelText('Latest arrival (optional)');
    fireEvent.change(arriveTo, { target: { value: '18:00' } });

    expect(screen.getByRole('button', { name: 'Add leg' })).not.toBeDisabled();
  });

  // Review §2.2/I17: states the rule up front rather than only via a
  // disabled button with no explanation.
  it('shows the at-least-one-of-four hint in window mode', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    expect(await screen.findByText('At least one of the four times below is required.')).toBeInTheDocument();
  });

  // Review §2.2/M14: same "no visible earliest > latest check" gap
  // TrackTrainForm's own window fields had.
  it('disables submit when the latest departure is before the earliest departure', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByLabelText('Destination CRS');
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    fireEvent.change(screen.getByLabelText('Service date'), { target: { value: '2026-09-22' } });
    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '18:00' } });
    fireEvent.change(screen.getByLabelText('Latest departure (optional)'), { target: { value: '09:00' } });

    expect(screen.getByRole('button', { name: 'Add leg' })).toBeDisabled();
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

  // `JourneyCreationFlow` (the `/journeys/new` continuous creation page)
  // reuses this button to chain leg 2+ onto a journey that only exists as
  // local client state there -- there is no server-rendered journey page
  // for `router.refresh()` to re-pull, so it needs the added leg's own
  // response instead of the refresh this button's other caller
  // (`app/journeys/[id]/page.tsx`) relies on.
  it('with onAdded: calls it with the add-leg response instead of router.refresh()', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(legResponse());
    const onAdded = vi.fn();

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" onAdded={onAdded} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByLabelText('Destination CRS');
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    const serviceDate = screen.getByLabelText('Service date');
    fireEvent.change(serviceDate, { target: { value: '2026-09-22' } });
    const departFrom = screen.getByLabelText('Earliest departure (optional)');
    fireEvent.change(departFrom, { target: { value: '09:00' } });

    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    await waitFor(() => expect(onAdded).toHaveBeenCalledWith({ legId: 9, trackingId: null }));
    expect(refreshMock).not.toHaveBeenCalled();
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
    const departFrom = screen.getByLabelText('Earliest departure (optional)');
    fireEvent.change(departFrom, { target: { value: '09:00' } });

    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    expect(await screen.findByText('no schedule matched that window')).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  // Bug: the Origin/Destination CRS and Service date fields were free text
  // with no client-side format check, so a malformed value used to sail
  // straight through to the backend and come back as a raw 400 instead of
  // a friendly inline message. `Add leg` also stays disabled -- the
  // existing `isValid` gate already blocks submission on any malformed
  // required field, this just adds the "why" underneath the field.
  it('shows an inline error for a malformed Origin CRS and keeps the button disabled', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const origin = await screen.findByLabelText('Origin CRS');
    fireEvent.change(origin, { target: { value: 'WOKX' } });

    expect(await screen.findByText('Must be a 3-letter CRS code')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add leg' })).toBeDisabled();
    expect(vi.mocked(fetch)).not.toHaveBeenCalled();
  });

  it('shows no CRS error while the Origin field is still empty', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    await screen.findByLabelText('Origin CRS');
    expect(screen.queryByText('Must be a 3-letter CRS code')).not.toBeInTheDocument();
  });

  it('shows an inline error for a malformed Service date and keeps the button disabled', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByLabelText('Destination CRS');
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    fireEvent.change(screen.getByLabelText('Service date'), { target: { value: '2026-02-30' } });
    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });

    expect(await screen.findByText('Must be a valid date (YYYY-MM-DD)')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add leg' })).toBeDisabled();
    expect(vi.mocked(fetch)).not.toHaveBeenCalled();
  });

  it('accepts a well-formed CRS and date and allows submission', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(legResponse());

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByLabelText('Destination CRS');
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    fireEvent.change(screen.getByLabelText('Service date'), { target: { value: '2026-09-22' } });
    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });

    expect(screen.queryByText('Must be a 3-letter CRS code')).not.toBeInTheDocument();
    expect(screen.queryByText('Must be a valid date (YYYY-MM-DD)')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add leg' })).not.toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
  });
});
