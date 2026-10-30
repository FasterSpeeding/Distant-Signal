import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { renderWithMantine } from '@/test/render';
import { theme } from '@/lib/theme';
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

const STATIONS = [
  { code: 'WAT', name: 'London Waterloo' },
  { code: 'CLJ', name: 'Clapham Junction' },
  { code: 'WOK', name: 'Woking' },
];

/** `GET /tfl/stops`'s rows, with invented ids. */
const TFL_STOPS = [
  { id: '940GZZTESTV', name: 'Victoria', modes: ['tube'], lines: [{ id: 'victoria', name: 'Victoria' }] },
  { id: '940GZZTESTE', name: 'Euston', modes: ['tube'], lines: [{ id: 'northern', name: 'Northern' }] },
];

/** Routes the mocked `fetch` by URL: the station fields' own type-ahead
 * search (`/api/stations?q=`, matched on code or name like the real API)
 * gets the `STATIONS` fixture, and the add-leg POST gets `legs()`. */
function mockFetch(legs: () => Response = legResponse) {
  const fetchMock = vi.mocked(fetch);
  fetchMock.mockImplementation((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.startsWith('/api/stations?')) {
      const q = (new URLSearchParams(url.split('?')[1]).get('q') ?? '').toLowerCase();
      const matches = STATIONS.filter((s) => s.code.toLowerCase() === q || s.name.toLowerCase().includes(q));
      return Promise.resolve(new Response(JSON.stringify(matches), { status: 200 }));
    }
    if (url.startsWith('/api/tfl/stops/rail-crs?')) {
      const ids = new URLSearchParams(url.split('?')[1]).get('ids') ?? '';
      const known: Record<string, string[]> = { '940GZZLUEUS': ['EUS'] };
      return Promise.resolve(new Response(JSON.stringify(ids in known ? { [ids]: known[ids] } : {}), { status: 200 }));
    }
    if (url.startsWith('/api/tfl/stops?')) {
      const params = new URLSearchParams(url.split('?')[1]);
      const q = (params.get('q') ?? '').toLowerCase();
      const modes = (params.get('modes') ?? '').split(',');
      const matches = TFL_STOPS.filter(
        (s) => s.name.toLowerCase().includes(q) && s.modes.some((mode) => modes.includes(mode)),
      );
      return Promise.resolve(new Response(JSON.stringify(matches), { status: 200 }));
    }
    return Promise.resolve(legs());
  });
  return fetchMock;
}

/** The add-leg POSTs made so far (station searches left out). */
function legPosts() {
  return vi.mocked(fetch).mock.calls.filter(([input]) => String(input).includes('/legs'));
}

/** A Tube place field: an `Autocomplete`, so its input is the combobox
 * (its listbox carries the same label). */
function placeField(name: string) {
  return screen.getByRole('combobox', { name });
}

describe('AddJourneyLegButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    mockFetch();
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("defaults the service date to London's today, not the host zone's (FE-4)", async () => {
    // FE-4: 23:30 UTC on 15 July is 00:30 on 16 July in London (BST).
    vi.useFakeTimers({ toFake: ['Date'], shouldAdvanceTime: true });
    vi.setSystemTime(new Date('2026-07-15T23:30:00Z'));
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));
    expect(await screen.findByLabelText('Service date')).toHaveValue('16 Jul 2026');
  });

  it("pre-fills the service date and earliest departure from the previous leg's arrival", async () => {
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs="WAT"
        priorArrival={{ date: '2026-09-22', time: '10:07' }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    expect(await screen.findByLabelText('Service date')).toHaveValue('22 Sep 2026');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('10:07');
    expect(screen.getByLabelText('Latest departure (optional)')).toHaveValue('');
    expect(screen.getByText(/Your last leg arrives at 10:07\./)).toBeInTheDocument();
  });

  it('pre-fills the next day for a previous leg that arrives after midnight', async () => {
    // The day is the arrival's own London date (`journeyPriorArrival`),
    // not today, even when today is the day the previous leg left.
    vi.useFakeTimers({ toFake: ['Date'], shouldAdvanceTime: true });
    vi.setSystemTime(new Date('2026-09-22T21:00:00Z'));
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs="WAT"
        priorArrival={{ date: '2026-09-23', time: '00:15' }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    expect(await screen.findByLabelText('Service date')).toHaveValue('23 Sep 2026');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('00:15');
  });

  it('leaves the earliest departure empty when there is no previous leg', async () => {
    vi.useFakeTimers({ toFake: ['Date'], shouldAdvanceTime: true });
    vi.setSystemTime(new Date('2026-09-22T09:00:00Z'));
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    expect(await screen.findByLabelText('Service date')).toHaveValue('22 Sep 2026');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('');
    expect(screen.queryByText(/Your last leg arrives/)).not.toBeInTheDocument();
  });

  it("leaves the earliest departure empty when the previous leg's arrival isn't known", async () => {
    vi.useFakeTimers({ toFake: ['Date'], shouldAdvanceTime: true });
    vi.setSystemTime(new Date('2026-09-22T09:00:00Z'));
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" priorArrival={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    expect(await screen.findByLabelText('Service date')).toHaveValue('22 Sep 2026');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('');
    expect(screen.queryByText(/Your last leg arrives/)).not.toBeInTheDocument();
  });

  it("keeps the traveller's own date and time when the dialog re-renders", async () => {
    const view = renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs="WAT"
        priorArrival={{ date: '2026-09-22', time: '10:07' }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const serviceDate = await screen.findByLabelText('Service date');
    fireEvent.change(serviceDate, { target: { value: '24 Sep 2026' } });
    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '12:30' } });

    // The journey refreshes under the open dialog with a later arrival.
    // (`rerender` replaces the whole tree, so the provider goes back in.)
    view.rerender(
      <MantineProvider theme={theme} env="test">
        <AddJourneyLegButton
          journeyId={1}
          priorDestinationCrs="WAT"
          priorArrival={{ date: '2026-09-22', time: '10:20' }}
        />
      </MantineProvider>,
    );

    expect(screen.getByLabelText('Service date')).toHaveValue('24 Sep 2026');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('12:30');

    fireEvent.change(screen.getByRole('combobox', { name: 'To station' }), { target: { value: 'CLJ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));
    await waitFor(() => expect(legPosts()).toHaveLength(1));
    expect(JSON.parse(String(legPosts()[0]?.[1]?.body))).toMatchObject({
      serviceDate: '2026-09-24',
      departWindow: { after: '12:30', before: null },
    });
  });

  it('pre-fills the origin field from priorDestinationCrs on open', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const origin = await screen.findByRole('combobox', { name: 'From station' });
    expect(origin).toHaveValue('London Waterloo (WAT)');
  });

  it('re-seeds the origin field from priorDestinationCrs on every open, not just the first', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const origin = await screen.findByRole('combobox', { name: 'From station' });
    fireEvent.change(origin, { target: { value: 'CLJ' } });
    expect(origin).toHaveValue('CLJ');

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => expect(screen.queryByRole('combobox', { name: 'From station' })).not.toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));
    const reopenedOrigin = await screen.findByRole('combobox', { name: 'From station' });
    expect(reopenedOrigin).toHaveValue('London Waterloo (WAT)');
  });

  it("after a Tube leg, starts the next train leg at the Tube stop's station, and the time at its arrival", async () => {
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs={null}
        priorStopId="940GZZLUEUS"
        priorPlaceName="Euston"
        priorArrival={{ date: '2026-10-21', time: '10:07' }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const origin = await screen.findByRole('combobox', { name: 'From station' });
    await waitFor(() => expect(origin).toHaveValue('EUS'));
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('10:07');
    expect(screen.getByLabelText('Service date')).toHaveValue('21 Oct 2026');
    expect(vi.mocked(fetch)).toHaveBeenCalledWith('/api/tfl/stops/rail-crs?ids=940GZZLUEUS', {});

    // The untracked mode starts from the place itself.
    fireEvent.click(screen.getByText('Tube, bus or walk'));
    expect(await screen.findByRole('combobox', { name: 'From (optional)' })).toHaveValue('Euston');
  });

  it('after a walk to a station by name, starts the next train leg there', async () => {
    renderWithMantine(
      <AddJourneyLegButton journeyId={1} priorDestinationCrs={null} priorPlaceName="Clapham Junction" />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));
    const origin = await screen.findByRole('combobox', { name: 'From station' });
    await waitFor(() => expect(origin).toHaveValue('Clapham Junction (CLJ)'));
  });

  it('after a walk to a place that is no station, opens on "Tube, bus or walk" from that place', async () => {
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs={null}
        priorPlaceName="Home"
        priorArrival={{ date: '2026-10-21', time: '10:07' }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    expect(await screen.findByRole('combobox', { name: 'From (optional)' })).toHaveValue('Home');
    expect(screen.getByRole('radio', { name: 'Tube, bus or walk' })).toBeChecked();
    expect(screen.getByLabelText('Leaves')).toHaveValue('10:07');
    expect(screen.queryByRole('combobox', { name: 'From station' })).not.toBeInTheDocument();
    expect(vi.mocked(fetch).mock.calls.some(([input]) => String(input) === '/api/stations?q=Home')).toBe(true);
  });

  it('after a leg that ends at a station, opens on the train search from it', async () => {
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs={null}
        priorStopId="940GZZLUEUS"
        priorPlaceName="Euston"
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    expect(await screen.findByRole('combobox', { name: 'From station' })).toHaveValue('EUS');
    expect(screen.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
  });

  it('opens once the station lookup answers, and ignores a second click meanwhile', async () => {
    let answer: (response: Response) => void = () => {};
    const fetchMock = mockFetch();
    const routed = fetchMock.getMockImplementation();
    fetchMock.mockImplementation((input: RequestInfo | URL, init?: RequestInit) =>
      String(input).startsWith('/api/tfl/stops/rail-crs?')
        ? new Promise<Response>((resolve) => {
            answer = resolve;
          })
        : routed!(input, init),
    );
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs={null}
        priorStopId="940GZZLUEUS"
        priorPlaceName="Euston"
      />,
    );
    const button = screen.getByRole('button', { name: 'Add a leg' });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(fetchMock.mock.calls.filter(([input]) => String(input).includes('rail-crs'))).toHaveLength(1);

    answer(new Response(JSON.stringify({ '940GZZLUEUS': ['EUS'] }), { status: 200 }));
    expect(await screen.findByRole('combobox', { name: 'From station' })).toHaveValue('EUS');
  });

  it('POSTs a window-mode request with the entered origin/destination/date/departWindow', async () => {
    const fetchMock = mockFetch(legResponse);

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByRole('combobox', { name: 'To station' });
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

    const destination = await screen.findByRole('combobox', { name: 'To station' });
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

    const destination = await screen.findByRole('combobox', { name: 'To station' });
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    fireEvent.change(screen.getByLabelText('Service date'), { target: { value: '2026-09-22' } });
    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '18:00' } });
    fireEvent.change(screen.getByLabelText('Latest departure (optional)'), { target: { value: '09:00' } });

    expect(screen.getByRole('button', { name: 'Add leg' })).toBeDisabled();
  });

  it('POSTs a knownTrain-mode request when the direct-pick mode is selected', async () => {
    const fetchMock = mockFetch(legResponse);

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

  // `JourneyCreationFlow` (the `/plan` continuous creation page)
  // reuses this button to chain leg 2+ onto a journey that only exists as
  // local client state there -- there is no server-rendered journey page
  // for `router.refresh()` to re-pull, so it needs the added leg's own
  // response instead of the refresh this button's other caller
  // (`app/journeys/[id]/page.tsx`) relies on.
  it('with onAdded: calls it with the add-leg response instead of router.refresh()', async () => {
    const onAdded = vi.fn();

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" onAdded={onAdded} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByRole('combobox', { name: 'To station' });
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
    mockFetch(() => new Response('no schedule matched that window', { status: 400 }));

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByRole('combobox', { name: 'To station' });
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    const serviceDate = screen.getByLabelText('Service date');
    fireEvent.change(serviceDate, { target: { value: '2026-09-22' } });
    const departFrom = screen.getByLabelText('Earliest departure (optional)');
    fireEvent.change(departFrom, { target: { value: '09:00' } });

    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    expect(await screen.findByText('No schedule matched that window.')).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it("shows a 422 for a train that can't serve the leg in the form's error alert", async () => {
    const sentence = "Train A12345 doesn't stop at Woking (WOK) on 22 Sep 2026.";
    mockFetch(() => new Response(sentence, { status: 422 }));

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));
    fireEvent.click(await screen.findByText('I know the train'));
    fireEvent.change(await screen.findByLabelText('Train UID'), { target: { value: 'A12345' } });
    fireEvent.change(screen.getByLabelText('Service date'), { target: { value: '2026-09-22' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(sentence);
    expect(refreshMock).not.toHaveBeenCalled();
    // The dialog stays open with the traveller's entry, to fix and resend.
    expect(screen.getByLabelText('Train UID')).toHaveValue('A12345');
  });

  // Bug: the Origin/Destination CRS fields (and, before it became a
  // `DateInput`, the Service date) were free text with no client-side
  // format check, so a malformed value used to sail
  // straight through to the backend and come back as a raw 400 instead of
  // a friendly inline message. `Add leg` also stays disabled -- the
  // existing `isValid` gate already blocks submission on any malformed
  // required field, this just adds the "why" underneath the field.
  it('shows an inline error for a malformed Origin CRS and keeps the button disabled', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const origin = await screen.findByRole('combobox', { name: 'From station' });
    fireEvent.change(origin, { target: { value: 'WOKX' } });
    fireEvent.blur(origin);

    expect(await screen.findByText('Must be a 3-letter station code')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add leg' })).toBeDisabled();
    expect(legPosts()).toHaveLength(0);
  });

  it('shows no station error while a name is still being typed', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const origin = await screen.findByRole('combobox', { name: 'From station' });
    fireEvent.change(origin, { target: { value: 'Wok' } });

    await waitFor(() => expect(vi.mocked(fetch)).toHaveBeenCalledWith('/api/stations?q=Wok', expect.anything()));
    expect(screen.queryByText('Must be a 3-letter station code')).not.toBeInTheDocument();
  });

  it('resolves a typed full station name to its code', async () => {
    const fetchMock = mockFetch(legResponse);

    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByRole('combobox', { name: 'To station' });
    fireEvent.change(destination, { target: { value: 'clapham junction' } });
    fireEvent.blur(destination);
    fireEvent.change(screen.getByLabelText('Service date'), { target: { value: '2026-09-22' } });
    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });

    // Picked once the search for what was typed comes back.
    await waitFor(() => expect(destination).toHaveValue('Clapham Junction (CLJ)'), { timeout: 5000 });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Add leg' })).not.toBeDisabled());
    expect(screen.queryByText('Must be a 3-letter station code')).not.toBeInTheDocument();
    expect(screen.getByText('Only trains reaching CLJ at or after this time.')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));
    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Journeys/1/legs',
        expect.objectContaining({ body: expect.stringContaining('"destinationCrs":"CLJ"') as unknown }),
      );
    });
  });

  it('fills in the code when a station is picked from the suggestions by name', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByRole('combobox', { name: 'To station' });
    fireEvent.focus(destination);
    fireEvent.change(destination, { target: { value: 'Clapham' } });
    fireEvent.click(await screen.findByRole('option', { name: 'CLJ · Clapham Junction', hidden: true }));

    expect(destination).toHaveValue('Clapham Junction (CLJ)');
  });

  it('shows no CRS error while the Origin field is still empty', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs={null} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    await screen.findByRole('combobox', { name: 'From station' });
    expect(screen.queryByText('Must be a 3-letter station code')).not.toBeInTheDocument();
  });

  // The Service date is a `DateInput` now, not free text: text that isn't
  // a date leaves the last good date in place (shown again on blur), so
  // nothing malformed can reach `POST /Journeys/{id}/legs`.
  it('keeps the last good Service date when the typed text is not a date', async () => {
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs="WAT"
        priorArrival={{ date: '2026-09-22', time: '10:07' }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByRole('combobox', { name: 'To station' });
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    const serviceDate = screen.getByLabelText('Service date');
    fireEvent.focus(serviceDate);
    fireEvent.change(serviceDate, { target: { value: 'next tuesday' } });
    fireEvent.blur(serviceDate);
    expect(serviceDate).toHaveValue('22 Sep 2026');

    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));
    await waitFor(() => expect(legPosts()).toHaveLength(1));
    expect(JSON.parse(String(legPosts()[0]?.[1]?.body))).toMatchObject({ serviceDate: '2026-09-22' });
  });

  it('accepts a well-formed CRS and date and allows submission', async () => {
    renderWithMantine(<AddJourneyLegButton journeyId={1} priorDestinationCrs="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    const destination = await screen.findByRole('combobox', { name: 'To station' });
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    fireEvent.change(screen.getByLabelText('Service date'), { target: { value: '2026-09-22' } });
    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });

    expect(screen.queryByText('Must be a 3-letter station code')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add leg' })).not.toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));
    await waitFor(() => expect(legPosts()).toHaveLength(1));
  });

  /** Spec 2026-10-20 §2: the "Tube, bus or walk" mode adds an untracked
   * leg, seeded from the previous leg's arrival and destination. */
  it('POSTs an untracked Tube leg seeded from the previous leg', async () => {
    const fetchMock = mockFetch(legResponse);
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs="EUS"
        priorArrival={{ date: '2026-10-21', time: '10:07' }}
        priorPlaceName="London Euston"
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));
    fireEvent.click(await screen.findByText('Tube, bus or walk'));

    expect(await screen.findByRole('combobox', { name: 'From (optional)' })).toHaveValue('London Euston');
    expect(screen.getByLabelText('Date')).toHaveValue('21 Oct 2026');
    expect(screen.getByLabelText('Leaves')).toHaveValue('10:07');
    expect(screen.queryByLabelText('Service date')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add leg' })).toBeDisabled();

    fireEvent.change(screen.getByLabelText('Line (optional)'), { target: { value: 'Victoria line' } });
    fireEvent.change(placeField('To (optional)'), { target: { value: 'Victoria' } });
    fireEvent.change(screen.getByLabelText('Arrives'), { target: { value: '10:25' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Journeys/1/legs',
        expect.objectContaining({
          method: 'POST',
          // BST: 10:07 and 10:25 London are 09:07 and 09:25 UTC.
          body: JSON.stringify({
            mode: 'untracked',
            transitMode: 'tube',
            lineName: 'Victoria line',
            originName: 'London Euston',
            destinationName: 'Victoria',
            plannedDeparture: '2026-10-21T09:07:00.000Z',
            plannedArrival: '2026-10-21T09:25:00.000Z',
          }),
        }),
      );
    });
  });

  it('fills a Tube place and its stop id from the TfL stop lookup, and an edit drops the id', async () => {
    const fetchMock = mockFetch(legResponse);
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs={null}
        priorArrival={{ date: '2026-10-21', time: '10:07' }}
        priorPlaceName={null}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));
    fireEvent.click(await screen.findByText('Tube, bus or walk'));

    // TfL's terms, beside the stop data.
    expect(
      screen.getByText(
        'Stop names: Powered by TfL Open Data. Contains OS data © Crown copyright and database rights 2016. Geomni UK Map data © and database rights 2019.',
      ),
    ).toBeInTheDocument();
    const to = placeField('To (optional)');
    fireEvent.change(to, { target: { value: 'vic' } });
    // `Victoria · Tube: Victoria`: its modes and lines after the name.
    fireEvent.click(await screen.findByRole('option', { name: 'Victoria · Tube: Victoria' }));
    expect(to).toHaveValue('Victoria');
    expect(fetchMock).toHaveBeenCalledWith('/api/tfl/stops?q=vic&modes=tube', expect.anything());

    const from = placeField('From (optional)');
    fireEvent.change(from, { target: { value: 'eus' } });
    fireEvent.click(await screen.findByRole('option', { name: 'Euston · Tube: Northern' }));
    expect(from).toHaveValue('Euston');
    // Edited after the pick: free text again, so no stop id.
    fireEvent.change(from, { target: { value: 'Euston (north entrance)' } });

    fireEvent.change(screen.getByLabelText('Arrives'), { target: { value: '10:25' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));
    await waitFor(() => expect(legPosts()).toHaveLength(1));
    const [, init] = legPosts()[0] as [string, RequestInit];
    expect(JSON.parse(String(init.body))).toEqual({
      mode: 'untracked',
      transitMode: 'tube',
      originName: 'Euston (north entrance)',
      destinationName: 'Victoria',
      destinationStopId: '940GZZTESTV',
      plannedDeparture: '2026-10-21T09:07:00.000Z',
      plannedArrival: '2026-10-21T09:25:00.000Z',
    });
  });

  it('after the last leg, opens on the time-window search leaving at or after its arrival', async () => {
    const fetchMock = mockFetch(legResponse);
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs="WAT"
        priorArrival={{ date: '2026-10-21', time: '10:07' }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg' }));

    expect(await screen.findByRole('radio', { name: 'Search a time window' })).toBeChecked();
    fireEvent.change(screen.getByRole('combobox', { name: 'To station' }), { target: { value: 'CLJ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Journeys/1/legs',
        expect.objectContaining({
          body: JSON.stringify({
            mode: 'window',
            originCrs: 'WAT',
            destinationCrs: 'CLJ',
            serviceDate: '2026-10-21',
            departWindow: { after: '10:07', before: null },
            arriveWindow: { after: null, before: null },
          }),
        }),
      );
    });
  });

  /** "Add a leg at the start" inserts in front of the first leg, and opens
   * on the same time-window search as "Add a leg", mirrored: a train
   * arriving where the first leg starts, at or before it leaves. */
  it('inserts before a leg on the time-window search, arriving where and before it starts', async () => {
    const fetchMock = mockFetch(legResponse);
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs={null}
        beforeLegId={7}
        nextLegStart={{ place: 'London Waterloo', crs: 'WAT', departure: { date: '2026-10-21', time: '09:30' } }}
        label="Add a leg at the start"
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg at the start' }));

    expect(await screen.findByRole('radio', { name: 'Search a time window' })).toBeChecked();
    expect(screen.getByRole('combobox', { name: 'From station' })).toHaveValue('');
    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'To station' })).toHaveValue('London Waterloo (WAT)'),
    );
    expect(screen.getByLabelText('Service date')).toHaveValue('21 Oct 2026');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('');
    expect(screen.getByLabelText('Latest arrival (optional)')).toHaveValue('09:30');
    expect(screen.getByText(/Your next leg leaves at 09:30\./)).toBeInTheDocument();

    fireEvent.change(screen.getByRole('combobox', { name: 'From station' }), { target: { value: 'WOK' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Journeys/1/legs?before=7',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({
            mode: 'window',
            originCrs: 'WOK',
            destinationCrs: 'WAT',
            serviceDate: '2026-10-21',
            departWindow: { after: null, before: null },
            arriveWindow: { after: null, before: '09:30' },
          }),
        }),
      );
    });
  });

  it("inserts before a Tube leg on the time-window search to its stop's station", async () => {
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs={null}
        beforeLegId={7}
        nextLegStart={{
          place: 'Euston',
          crs: null,
          stopId: '940GZZLUEUS',
          departure: { date: '2026-10-21', time: '09:30' },
        }}
        label="Add a leg at the start"
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg at the start' }));

    expect(await screen.findByRole('combobox', { name: 'To station' })).toHaveValue('EUS');
    expect(screen.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
    expect(screen.getByLabelText('Latest arrival (optional)')).toHaveValue('09:30');
    expect(vi.mocked(fetch)).toHaveBeenCalledWith('/api/tfl/stops/rail-crs?ids=940GZZLUEUS', {});
  });

  /** Spec 2026-10-20 §2.3 follow-up: a first leg that starts somewhere no
   * station is ("Home") still opens on "Tube, bus or walk", ending where
   * and when that leg starts. */
  it('inserts an untracked leg before a leg that starts at no station, seeded to end where it starts', async () => {
    const fetchMock = mockFetch(legResponse);
    renderWithMantine(
      <AddJourneyLegButton
        journeyId={1}
        priorDestinationCrs={null}
        beforeLegId={7}
        nextLegStart={{ place: 'Home', crs: null, departure: { date: '2026-10-21', time: '09:30' } }}
        label="Add a leg at the start"
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add a leg at the start' }));
    expect(await screen.findByRole('combobox', { name: 'To (optional)' })).toHaveValue('Home');
    expect(screen.getByRole('radio', { name: 'Tube, bus or walk' })).toBeChecked();
    expect(screen.getByLabelText('Date')).toHaveValue('21 Oct 2026');
    expect(screen.getByLabelText('Arrives')).toHaveValue('09:30');
    fireEvent.change(placeField('From (optional)'), { target: { value: 'Brixton' } });
    fireEvent.change(screen.getByLabelText('Leaves'), { target: { value: '09:05' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Journeys/1/legs?before=7',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({
            mode: 'untracked',
            transitMode: 'tube',
            originName: 'Brixton',
            destinationName: 'Home',
            plannedDeparture: '2026-10-21T08:05:00.000Z',
            plannedArrival: '2026-10-21T08:30:00.000Z',
          }),
        }),
      );
    });
  });
});
