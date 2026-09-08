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
 * ENVELOPE, not a bare array (Task 7): `results` plus a `nextCursor` that
 * is an explicit `null` on the last page. Every test that stubs a search
 * response goes through this, so no test can accidentally assert against
 * the pre-pagination bare-array shape. */
function searchBody(
  rows: Array<{ uid: string; scheduled: string; originCrs: string; destinationCrs: string }>,
  nextCursor: string | null = null,
) {
  return JSON.stringify({ results: rows, nextCursor });
}

const PAGE_ONE = [
  { uid: 'C10001', scheduled: '08:22', originCrs: 'EUS', destinationCrs: 'MAN' },
  { uid: 'C10002', scheduled: '10:05', originCrs: 'CRE', destinationCrs: 'MAN' },
];
const PAGE_TWO = [
  { uid: 'C10003', scheduled: '11:40', originCrs: 'EUS', destinationCrs: 'MAN' },
];
const PAGE_THREE = [
  { uid: 'C10004', scheduled: '13:15', originCrs: 'CRE', destinationCrs: 'MAN' },
];

/** Routes a mocked `fetch` by URL: the search call, the station-suggestion
 * calls both Autocompletes fire, and the track/attach calls
 * `TrackThisTrainButton` makes. `search` defaults to two rows and no next
 * page, so most tests only override the branch they care about.
 *
 * `search` receives the request URL so a test can answer page 1 and page 2
 * differently -- which is exactly what "Load more" needs to be tested
 * honestly. */
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

  it('does not search until a valid destination CRS is entered', () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm />);

    expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    expect(
      screen.getByText('Enter a destination station above to search for trains.'),
    ).toBeInTheDocument();
  });

  it('sends only the destination when no optional filter is set', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?destination=MAN'));
  });

  it('sends every optional filter it has, uppercased', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="man" initialOrigin="eus" />);

    fireEvent.change(screen.getByLabelText('From (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?destination=MAN&origin=EUS&from=09%3A00&to=12%3A00',
      ),
    );
  });

  it('renders one row per result, with time, origin and destination', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('08:22 · EUS → MAN')).toBeInTheDocument();
    expect(screen.getByText('10:05 · CRE → MAN')).toBeInTheDocument();
  });

  it('links each row to the public train page for today', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    const links = await screen.findAllByRole('link', { name: 'View live status' });
    const today = new Date().toISOString().slice(0, 10);
    expect(links[0]).toHaveAttribute('href', `/train/C10001/${today}`);
  });

  it('renders a Track this train action on every row', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    expect(buttons).toHaveLength(2);
  });

  it("passes attachTicketId through, so the row's track action attaches the ticket", async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" attachTicketId={7} />);

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

  // The 404-vs-200-[] split the backend route draws deliberately (Task 7)
  // has to survive into the UI, or it was pointless.
  it('distinguishes "nothing published for this destination" from "no matches"', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('not found', { status: 404 }) }));
    renderWithMantine(<TrainSearchForm initialDestination="ZZZ" />);

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
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText('No scheduled trains match those filters right now.'),
    ).toBeInTheDocument();
  });

  it('shows an error state on a 500', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('boom', { status: 500 }) }));
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText("Couldn't search for trains right now. Try again."),
    ).toBeInTheDocument();
  });

  // The honesty requirement carried over from TrackTrainForm's own CIF
  // branch: these rows are timetable data, not live running information,
  // and the UI must never imply otherwise.
  it('labels the results as scheduled timetable data, not live status', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

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

  // ---- Pagination. There is no cap anywhere in the backend any more, so
  // a busy destination genuinely has more trains than one page; "Load more"
  // is how the user reaches them, and these four tests are the contract.

  it('does not offer Load more when the response has no nextCursor', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('08:22 · EUS → MAN')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('offers Load more when the response carries a nextCursor', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: () => new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
      }),
    );
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByRole('button', { name: 'Load more' })).toBeInTheDocument();
  });

  it('appends the next page rather than replacing the rows, and sends after=', async () => {
    // The load-bearing assertion of the whole pagination change: APPEND.
    // A "Load more" that replaced the list would look like it worked while
    // silently losing page 1.
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=CURSOR1')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('11:40 · EUS → MAN')).toBeInTheDocument();
    expect(
      screen.getByText('08:22 · EUS → MAN'),
      'page 1 must still be on screen -- Load more appends, it does not replace',
    ).toBeInTheDocument();
    expect(screen.getByText('10:05 · CRE → MAN')).toBeInTheDocument();

    const urls = searchCallUrls(fetchMock);
    expect(urls).toHaveLength(2);
    expect(urls[0]).toBe('/api/trains/search?destination=MAN');
    expect(urls[1]).toBe('/api/trains/search?destination=MAN&after=CURSOR1');

    // Exhausted: the second response's nextCursor was null.
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument(),
    );
  });

  it('uses the NEW cursor on a second Load more, not the first one again', async () => {
    // Guards the specific bug an append-only implementation invites:
    // keeping the cursor from the original search in state and re-sending
    // it, which would fetch page 2 forever and duplicate its rows.
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
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    expect(await screen.findByText('11:40 · EUS → MAN')).toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('13:15 · CRE → MAN')).toBeInTheDocument();

    const urls = searchCallUrls(fetchMock);
    expect(urls).toHaveLength(3);
    expect(urls[1]).toBe('/api/trains/search?destination=MAN&after=CURSOR1');
    expect(
      urls[2],
      'the second Load more must use the cursor from the SECOND response',
    ).toBe('/api/trains/search?destination=MAN&after=CURSOR2');
    expect(screen.getAllByText('11:40 · EUS → MAN')).toHaveLength(1);
  });

  it('keeps the original filters on a Load more request', async () => {
    // The cursor is positional, not self-describing: dropping `origin`
    // or the time range on page 2 would silently widen the search
    // mid-scroll.
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="man" initialOrigin="eus" />);

    fireEvent.change(screen.getByLabelText('From (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    await waitFor(() => expect(searchCallUrls(fetchMock)).toHaveLength(2));
    expect(searchCallUrls(fetchMock)[1]).toBe(
      '/api/trains/search?destination=MAN&origin=EUS&from=09%3A00&to=12%3A00&after=CURSOR1',
    );
  });

  it('starts a fresh search over rather than appending to the previous one', async () => {
    // Pressing Search again after paginating must RESET, not append -- the
    // opposite of Load more. Same append-vs-replace bug, mirrored.
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    expect(await screen.findByText('11:40 · EUS → MAN')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(screen.queryByText('11:40 · EUS → MAN')).not.toBeInTheDocument(),
    );
    expect(screen.getByText('08:22 · EUS → MAN')).toBeInTheDocument();
  });
});
