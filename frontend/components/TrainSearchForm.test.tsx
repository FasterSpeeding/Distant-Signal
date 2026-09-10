import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrainSearchForm } from './TrainSearchForm';

/** A small, fixed, real-station-shaped dataset backing every autocomplete
 * field in this file (Station/Departing from/Stops at all share the same
 * `useSuggestions` hook). Mocking the HOOK itself, rather than driving the
 * real 250ms-debounced fetch behind it, keeps these tests synchronous and
 * deterministic -- `useSuggestions` itself is this codebase's shared,
 * separately-relied-on piece of plumbing (`lib/useSuggestions.ts`), not
 * something this component's own tests need to re-prove. Case-insensitive
 * substring match against `code`/`name`, mirroring the real
 * `/api/stations?q=` endpoint's own shape closely enough for these tests'
 * purposes. */
const TEST_STATIONS = [
  { code: 'RDG', name: 'Reading' },
  { code: 'OXF', name: 'Oxford' },
];

vi.mock('@/lib/useSuggestions', () => ({
  useSuggestions: (query: string) => {
    const q = query.trim().toLowerCase();
    const suggestions = q
      ? TEST_STATIONS.filter((s) => s.code.toLowerCase().includes(q) || s.name.toLowerCase().includes(q))
      : [];
    return { suggestions, loading: false };
  },
}));

/** The `TagsInput`/`Autocomplete` field's own text input, found by
 * `role: 'combobox'`, not `getByLabelText` -- Mantine's `Combobox`-based
 * inputs also point their DROPDOWN LISTBOX's `aria-labelledby` at the same
 * label, so a plain `getByLabelText` matches both the input and the
 * (empty, hidden) listbox and throws "multiple elements found". */
function stopsAtInput() {
  return screen.getByRole('combobox', { name: 'Stops at (optional)' });
}

/** Types `text` into the Stops at field and presses Enter to attempt
 * committing it as a chip. `fireEvent.click` first, not `fireEvent.focus`
 * -- Mantine's `Combobox`-based inputs reconcile typed text against the
 * ACTUAL focused element (`isExternalInputChange`), and `fireEvent.focus`
 * alone does not move jsdom's `document.activeElement` the way a real
 * click's default action does. */
function typeAndCommit(text: string) {
  const input = stopsAtInput();
  fireEvent.click(input);
  fireEvent.change(input, { target: { value: text } });
  fireEvent.keyDown(input, { key: 'Enter' });
}

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

// See TrackTrainForm.test.tsx's identical mock (lines 62-97) for why a
// thin stand-in is used instead of driving the real popover calendar:
// fireEvent.change needs a real <input>, and DatePickerInput's real
// control isn't one. Keeps the same onChange(string | null) contract
// TrainSearchForm actually depends on.
vi.mock('@mantine/dates', () => ({
  DatePickerInput: ({
    label,
    value,
    onChange,
    description,
  }: {
    label: string;
    value: string | null;
    onChange: (value: string | null) => void;
    description?: string;
  }) => (
    <div>
      <label htmlFor="test-search-date">{label}</label>
      <input
        id="test-search-date"
        value={value ?? ''}
        onChange={(event) => onChange(event.target.value || null)}
      />
      {description && <p>{description}</p>}
    </div>
  ),
}));

/** Builds a `GET /public/trains/search` response body. The route returns an
 * ENVELOPE, not a bare array: `results` plus a `nextCursor` that is an
 * explicit `null` on the last page. */
function searchBody(
  rows: Array<{
    uid: string;
    scheduled: string;
    stationCrs: string;
    originCrs: string | null;
    destinationCrs: string | null;
    destinationArrival?: string | null;
  }>,
  nextCursor: string | null = null,
) {
  return JSON.stringify({
    results: rows.map((row) => ({ destinationArrival: null, ...row })),
    nextCursor,
  });
}

const PAGE_ONE = [
  { uid: 'C10001', scheduled: '08:22', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT' },
  { uid: 'C10002', scheduled: '10:05', stationCrs: 'MAN', originCrs: 'CRE', destinationCrs: 'WAT' },
];
const PAGE_TWO = [
  { uid: 'C10003', scheduled: '11:40', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT' },
];
const PAGE_THREE = [
  { uid: 'C10004', scheduled: '13:15', stationCrs: 'MAN', originCrs: 'CRE', destinationCrs: 'WAT' },
];

function mockFetchByUrl(
  options: { search?: (url: string) => Response; track?: () => Response } = {},
) {
  const {
    search = () => new Response(searchBody(PAGE_ONE), { status: 200 }),
    track = () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
  } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.startsWith('/api/trains/search')) return Promise.resolve(search(url));
    if (url.startsWith('/api/stations?')) return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    if (/\/api\/Train\/tickets\/\d+\/attach$/.test(url))
      return Promise.resolve(new Response(JSON.stringify({ ticketId: 7, trackedTrainId: 42 }), { status: 200 }));
    if (/\/api\/Train\/by-uid\/.+\/track$/.test(url)) return Promise.resolve(track());
    throw new Error(`unexpected fetch for ${url}`);
  });
}

function searchCallUrls(fetchMock: ReturnType<typeof vi.fn>): string[] {
  return fetchMock.mock.calls
    .map((args: unknown[]) => String(args[0]))
    .filter((url: string) => url.startsWith('/api/trains/search'));
}

function searchCallUrl(fetchMock: ReturnType<typeof vi.fn>): string {
  const urls = searchCallUrls(fetchMock);
  if (urls.length === 0) throw new Error('no /api/trains/search call recorded');
  return urls[0];
}

describe('TrainSearchForm', () => {
  beforeEach(() => {
    pushMock.mockClear();
  });

  it('does not search until a valid station CRS is entered', () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm />);

    expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    expect(
      screen.getByText('Enter a station above to search for trains that call there.'),
    ).toBeInTheDocument();
  });

  it('sends only the station when no optional filter is set', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN'));
  });

  it('includes the selected date in the search request', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.change(screen.getByLabelText('Date (optional)'), { target: { value: '2026-09-16' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&date=2026-09-16'),
    );
  });

  it('omits date from the search request when no date is picked', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN'));
  });

  it('sends every optional filter it has, uppercased', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="man" initialOrigin="eus" initialStopsAt={['OXF']} />);

    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('Latest departure (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&origin=EUS&stops_at=OXF&from=09%3A00&to=12%3A00',
      ),
    );
  });

  it('renders one row per result, with time, origin and destination', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('08:22 · EUS → MAN → WAT')).toBeInTheDocument();
    expect(screen.getByText('10:05 · CRE → MAN → WAT')).toBeInTheDocument();
  });

  it('renders a "?" placeholder when origin or destination is unknown', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: () =>
          new Response(
            searchBody([
              { uid: 'C99999', scheduled: '09:00', stationCrs: 'MAN', originCrs: null, destinationCrs: 'WAT' },
            ]),
            { status: 200 },
          ),
      }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('09:00 · ? → MAN → WAT')).toBeInTheDocument();
  });

  it('links each row to the public train page for today', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    const links = await screen.findAllByRole('link', { name: 'View live status' });
    const today = new Date().toISOString().slice(0, 10);
    expect(links[0]).toHaveAttribute('href', `/train/C10001/${today}`);
  });

  it('renders a Track this train action on every row', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    expect(buttons).toHaveLength(2);
  });

  it("passes attachTicketId through, so the row's track action attaches the ticket", async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    fireEvent.click(buttons[0]);

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/tickets/7/attach',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trackingId: 42 }) }),
      ),
    );
  });

  it('distinguishes "nothing published for today" from "no matches"', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('not found', { status: 404 }) }));
    renderWithMantine(<TrainSearchForm initialStation="ZZZ" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText(
        /Today's scheduled timetable data isn't available yet/,
      ),
    ).toBeInTheDocument();
  });

  it('says so when the search succeeds but matches nothing', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({ search: () => new Response(searchBody([]), { status: 200 }) }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText('No scheduled trains match those filters right now.'),
    ).toBeInTheDocument();
  });

  it('shows an error state on a 500', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('boom', { status: 500 }) }));
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText("Couldn't search for trains right now. Try again."),
    ).toBeInTheDocument();
  });

  it('labels the results as scheduled timetable data, not live status', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText(/scheduled timetable, not live running information/),
    ).toBeInTheDocument();
  });

  it('offers the manual /track fallback, carrying any ticketId through', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm attachTicketId={7} />);

    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute(
      'href',
      '/track?ticketId=7',
    );
  });

  it('offers the manual /track fallback with no query string when there is no ticketId', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute('href', '/track');
  });

  it('renders no operator filter at all', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.queryByLabelText(/Operator/i)).not.toBeInTheDocument();
  });

  it('does not offer Load more when the response has no nextCursor', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('08:22 · EUS → MAN → WAT')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('offers Load more when the response carries a nextCursor', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: () => new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
      }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByRole('button', { name: 'Load more' })).toBeInTheDocument();
  });

  it('appends the next page rather than replacing the rows, and sends after=', async () => {
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=CURSOR1')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('11:40 · EUS → MAN → WAT')).toBeInTheDocument();
    expect(
      screen.getByText('08:22 · EUS → MAN → WAT'),
      'page 1 must still be on screen -- Load more appends, it does not replace',
    ).toBeInTheDocument();
    expect(screen.getByText('10:05 · CRE → MAN → WAT')).toBeInTheDocument();

    const urls = searchCallUrls(fetchMock);
    expect(urls).toHaveLength(2);
    expect(urls[0]).toBe('/api/trains/search?station=MAN');
    expect(urls[1]).toBe('/api/trains/search?station=MAN&after=CURSOR1');

    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument(),
    );
  });

  it('uses the NEW cursor on a second Load more, not the first one again', async () => {
    const fetchMock = mockFetchByUrl({
      search: (url) => {
        if (url.includes('after=CURSOR2'))
          return new Response(searchBody(PAGE_THREE, null), { status: 200 });
        if (url.includes('after=CURSOR1'))
          return new Response(searchBody(PAGE_TWO, 'CURSOR2'), { status: 200 });
        return new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 });
      },
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    expect(await screen.findByText('11:40 · EUS → MAN → WAT')).toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('13:15 · CRE → MAN → WAT')).toBeInTheDocument();

    const urls = searchCallUrls(fetchMock);
    expect(urls).toHaveLength(3);
    expect(urls[1]).toBe('/api/trains/search?station=MAN&after=CURSOR1');
    expect(
      urls[2],
      'the second Load more must use the cursor from the SECOND response',
    ).toBe('/api/trains/search?station=MAN&after=CURSOR2');
    expect(screen.getAllByText('11:40 · EUS → MAN → WAT')).toHaveLength(1);
  });

  it('keeps the original filters on a Load more request', async () => {
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="man" initialOrigin="eus" />);

    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('Latest departure (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    await waitFor(() => expect(searchCallUrls(fetchMock)).toHaveLength(2));
    expect(searchCallUrls(fetchMock)[1]).toBe(
      '/api/trains/search?station=MAN&origin=EUS&from=09%3A00&to=12%3A00&after=CURSOR1',
    );
  });

  it('starts a fresh search over rather than appending to the previous one', async () => {
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    expect(await screen.findByText('11:40 · EUS → MAN → WAT')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(screen.queryByText('11:40 · EUS → MAN → WAT')).not.toBeInTheDocument(),
    );
    expect(screen.getByText('08:22 · EUS → MAN → WAT')).toBeInTheDocument();
  });

  it('adds a chip when the typed text matches a real station suggestion', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    typeAndCommit('RDG');

    expect(screen.getByText('RDG')).toBeInTheDocument();
  });

  it('does not add a chip for text that matches no real station suggestion', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    typeAndCommit('ZZZ');

    expect(screen.queryByText('ZZZ')).not.toBeInTheDocument();
  });

  it('adds multiple validated chips and sends one stops_at per chip', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    typeAndCommit('RDG');
    typeAndCommit('OXF');
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&stops_at=RDG&stops_at=OXF'),
    );
  });

  it('removes a chip via Backspace on the empty stops-at input', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    typeAndCommit('RDG');
    fireEvent.keyDown(stopsAtInput(), { key: 'Backspace' });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN'));
  });

  it('does not render the arrival-time filter until exactly one station is in Stops at', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    expect(screen.queryByLabelText('Earliest arrival (optional)')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Latest arrival (optional)')).not.toBeInTheDocument();
  });

  it('renders the arrival-time filter once exactly one station is in Stops at', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" initialStopsAt={['WAT']} />);

    expect(screen.getByLabelText('Earliest arrival (optional)')).toBeInTheDocument();
    expect(screen.getByLabelText('Latest arrival (optional)')).toBeInTheDocument();
  });

  it('hides the arrival-time filter again once a second stop is added', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" initialStopsAt={['RDG']} />);
    expect(screen.getByLabelText('Earliest arrival (optional)')).toBeInTheDocument();

    typeAndCommit('OXF');

    expect(screen.queryByLabelText('Earliest arrival (optional)')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Latest arrival (optional)')).not.toBeInTheDocument();
  });

  it('sends arrival_from/arrival_to only when stops_at names exactly one station', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="man" initialStopsAt={['WAT']} />);

    fireEvent.change(screen.getByLabelText('Earliest arrival (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('Latest arrival (optional)'), { target: { value: '09:30' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&stops_at=WAT&arrival_from=09%3A00&arrival_to=09%3A30',
      ),
    );
  });

  it('drops any previously-entered arrival-time filter once a second stop is added', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" initialStopsAt={['RDG']} />);

    fireEvent.change(screen.getByLabelText('Earliest arrival (optional)'), { target: { value: '09:00' } });
    typeAndCommit('OXF');
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&stops_at=RDG&stops_at=OXF'),
    );
  });

});
