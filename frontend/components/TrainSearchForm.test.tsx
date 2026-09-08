import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrainSearchForm } from './TrainSearchForm';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

/** Builds a `GET /public/trains/search` response body. The route returns an
 * ENVELOPE, not a bare array: `results` plus a `nextCursor` that is an
 * explicit `null` on the last page. */
function searchBody(
  rows: Array<{ uid: string; scheduled: string; stationCrs: string; originCrs: string | null; destinationCrs: string | null }>,
  nextCursor: string | null = null,
) {
  return JSON.stringify({ results: rows, nextCursor });
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

  it('sends every optional filter it has, uppercased', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="man" initialOrigin="eus" initialDestination="wat" />);

    fireEvent.change(screen.getByLabelText('From (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&origin=EUS&destination=WAT&from=09%3A00&to=12%3A00',
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

  it('renders no date filter at all', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.queryByLabelText(/^Date/i)).not.toBeInTheDocument();
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

    fireEvent.change(screen.getByLabelText('From (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '12:00' } });
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
});
