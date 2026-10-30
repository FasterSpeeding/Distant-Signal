import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrainSearchForm } from './TrainSearchForm';
import { nowInLondon } from '@/lib/londonWallClock';

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

const pushMock = vi.fn();
const replaceMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, replace: replaceMock }),
  usePathname: () => '/trains',
  // `null`, as outside the app router: the form falls back to its props.
  useSearchParams: () => null,
}));

// A PARTIAL mock: only `DatePickerInput` is stubbed out, and everything
// else `@mantine/dates` exports -- notably `TimeInput`, which backs this
// form's four time filters -- comes through as the real component. See
// TrackTrainForm.test.tsx's identical `DatePickerInput` mock (lines 62-97)
// for why that one specifically needs a thin stand-in: fireEvent.change
// needs a real <input>, and DatePickerInput's real control isn't one. Keeps
// the same onChange(string | null) contract TrainSearchForm depends on.
//
// `TimeInput` deliberately is NOT stubbed: it is a thin wrapper over a real
// native `<input type="time">`, so it already IS a real input that
// fireEvent.change drives, and stubbing it would leave these tests
// asserting against a hand-written stand-in rather than the control the
// page actually renders.
vi.mock('@mantine/dates', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@mantine/dates')>()),
  DatePickerInput: ({
    label,
    value,
    onChange,
    description,
    minDate,
    maxDate,
  }: {
    label: string;
    value: string | null;
    onChange: (value: string | null) => void;
    description?: string;
    minDate?: string;
    maxDate?: string;
  }) => (
    <div>
      <label htmlFor="test-search-date">{label}</label>
      <input
        id="test-search-date"
        value={value ?? ''}
        data-min-date={minDate}
        data-max-date={maxDate}
        onChange={(event) => onChange(event.target.value || null)}
      />
      {description && <p>{description}</p>}
    </div>
  ),
}));

/** A result row's link name starts with its time and destination
 * ("08:22 to WAT"), the visually hidden "to" included. */
function rowName(time: string, destination: string): RegExp {
  return new RegExp(`^${time} to ${destination}\\b`);
}

/** Builds a `GET /public/trains/search` response body. The route returns an
 * ENVELOPE, not a bare array: `results` plus a `nextCursor` that is an
 * explicit `null` on the last page. */
function searchBody(
  rows: {
    uid: string;
    scheduled: string;
    stationCrs: string;
    originCrs: string | null;
    destinationCrs: string | null;
    destinationName?: string | null;
    destinationArrival?: string | null;
    operator?: string | null;
    dayOffset?: number;
    serviceDate?: string;
  }[],
  nextCursor: string | null = null,
) {
  return JSON.stringify({
    results: rows.map((row) => ({ destinationArrival: null, destinationName: null, ...row })),
    nextCursor,
  });
}

const PAGE_ONE = [
  { uid: 'C10001', scheduled: '08:22', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT' },
  { uid: 'C10002', scheduled: '10:05', stationCrs: 'MAN', originCrs: 'CRE', destinationCrs: 'WAT' },
];
const PAGE_TWO = [{ uid: 'C10003', scheduled: '11:40', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT' }];
const PAGE_THREE = [{ uid: 'C10004', scheduled: '13:15', stationCrs: 'MAN', originCrs: 'CRE', destinationCrs: 'WAT' }];

function mockFetchByUrl(options: { search?: (url: string) => Response; track?: () => Response } = {}) {
  const {
    search = () => new Response(searchBody(PAGE_ONE), { status: 200 }),
    track = () =>
      new Response(JSON.stringify({ journeyId: 99, legId: 1, trackingId: 42, resolutionStatus: 'pending' }), {
        status: 200,
      }),
  } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.startsWith('/api/trains/search')) return Promise.resolve(search(url));
    if (url.startsWith('/api/stations?')) return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    // Every row's `TrackThisTrainButton` now prefetches the caller's groups
    // (`useGroupSummaries`) on mount -- see TrackThisTrainButton.test.tsx's
    // own `mockFetchByUrl` for the same addition. Empty-array 200: this
    // file's tests are only about search/pagination/track-action wiring,
    // never about the shared-groups prompt.
    if (url === '/api/groups') return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    if (/\/api\/Train\/tickets\/\d+\/attach$/.test(url))
      return Promise.resolve(new Response(JSON.stringify({ ticketId: 7, trackedTrainId: 42 }), { status: 200 }));
    // `TrackThisTrainButton` now POSTs `/api/Journeys` (a `knownTrain`-mode
    // leg) instead of the old by-uid track route -- see
    // TrackThisTrainButton.test.tsx's own `mockFetchByUrl` for the same
    // rewire.
    if (url.endsWith('/api/Journeys')) return Promise.resolve(track());
    throw new Error(`unexpected fetch for ${url}`);
  });
}

function searchCallUrls(fetchMock: ReturnType<typeof vi.fn>): string[] {
  return fetchMock.mock.calls
    .map((args: unknown[]) => String(args[0]))
    .filter((url: string) => url.startsWith('/api/trains/search'));
}

/** The LAST recorded `/api/trains/search` call, not the first. A valid
 * `initialStation` now fires an automatic mount-time search (see this
 * form's own mount `useEffect` and `clickSearch`/`awaitMountSettled` below)
 * ahead of anything a test does explicitly, so "the query that was actually
 * searched" -- what almost every test in this file cares about -- is
 * whichever call happened MOST recently, not necessarily the first one on
 * record. */
function searchCallUrl(fetchMock: ReturnType<typeof vi.fn>): string {
  const urls = searchCallUrls(fetchMock);
  if (urls.length === 0) throw new Error('no /api/trains/search call recorded');
  return urls[urls.length - 1]!;
}

/** Runs a search. Before the first search the form is the full form with a
 * Search button; from then on (and from the start when `initialStation` is
 * a valid code) it is the compact bar, which searches live with no button,
 * so there is nothing to press: this then just waits for the live search
 * to land. */
async function clickSearch() {
  const button = screen.queryByRole('button', { name: 'Search' });
  if (button) fireEvent.click(button);
  await searchSettled();
}

/** Waits for the search in flight to land: the spinner that stands in for
 * the Station field's search glyph goes away. */
async function searchSettled() {
  await waitFor(() => expect(screen.queryByTestId('list-filter-pending')).not.toBeInTheDocument());
}

/** Waits out the mount-time search (a valid `initialStation`), for a test
 * that makes its assertions synchronously afterwards. */
async function awaitMountSettled() {
  await searchSettled();
}

/** Opens the compact bar's Filters panel, if the form is the compact bar
 * and the panel is shut, so a field other than Station can be reached. */
function openFiltersIfCompact() {
  const button = screen.queryByRole('button', { name: /^Filters/ });
  if (button && button.getAttribute('aria-expanded') !== 'true') fireEvent.click(button);
}

/** The full form (before any search) with `MAN` typed into Station, so
 * the Search button's enabled state can be checked. */
function renderFullForm(props: Partial<Parameters<typeof TrainSearchForm>[0]> = {}) {
  const result = renderWithMantine(<TrainSearchForm {...props} />);
  fireEvent.change(screen.getByRole('combobox', { name: 'Station' }), { target: { value: 'MAN' } });
  return result;
}

/** Changes the date, which starts a fresh live search. */
function changeDate(value: string) {
  fireEvent.change(panelField('Date (optional)'), { target: { value } });
}

/** A field by its label, opening the Filters panel first if need be. */
function panelField(label: string): HTMLElement {
  openFiltersIfCompact();
  return screen.getByLabelText(label);
}

function panelCombobox(name: string): HTMLElement {
  openFiltersIfCompact();
  return screen.getByRole('combobox', { name });
}

describe('TrainSearchForm', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  beforeEach(() => {
    pushMock.mockClear();
    replaceMock.mockClear();
  });

  it('does not search until a valid station CRS is entered', () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm />);

    expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    expect(screen.getByText('Pick a station to see its departures.')).toBeInTheDocument();
  });

  it('invites a schedule lookup in every view, never a journey', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    const { unmount } = renderWithMantine(<TrainSearchForm initialBoard="arrivals" />);
    expect(screen.getByText('Pick a station to see its arrivals.')).toBeInTheDocument();
    unmount();
    renderWithMantine(<TrainSearchForm view="between" />);
    expect(screen.getByText('Pick two stations to see the trains between them.')).toBeInTheDocument();
    expect(screen.queryByText(/journey/i)).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /track/i })).not.toBeInTheDocument();
  });

  it('sends only the station when no optional filter is set', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&across_midnight=true'));
  });

  it('includes the selected date in the search request', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.change(panelField('Date (optional)'), { target: { value: '2026-09-16' } });
    await clickSearch();

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&date=2026-09-16'));
  });

  describe('date picker bounds', () => {
    const today = nowInLondon();
    const day = (offset: number) => today.add(offset, 'day').format('YYYY-MM-DD');

    it('come from searchDates when the page has them', async () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(
        <TrainSearchForm searchDates={{ from: day(-7), to: day(28), publishedFrom: day(-1), publishedTo: day(28) }} />,
      );
      const input = panelField('Date (optional)');
      expect(input).toHaveAttribute('data-min-date', day(-7));
      expect(input).toHaveAttribute('data-max-date', day(28));
      expect(screen.getByText('Search a different day, up to 7 days back and 28 days ahead.')).toBeInTheDocument();
    });

    it('fall back to a week either side of London today without them', async () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<TrainSearchForm searchDates={null} />);
      const input = panelField('Date (optional)');
      expect(input).toHaveAttribute('data-min-date', day(-7));
      expect(input).toHaveAttribute('data-max-date', day(7));
      expect(screen.getByText('Search a different day, up to a week either side of today.')).toBeInTheDocument();
    });
  });

  describe('provisional timetable note', () => {
    function provisionalBody(rows: typeof PAGE_ONE, nextCursor: string | null, provisional?: boolean) {
      const body = JSON.parse(searchBody(rows, nextCursor)) as Record<string, unknown>;
      return JSON.stringify(provisional === undefined ? body : { ...body, provisional, provisionalFrom: '2026-10-16' });
    }

    it('says the timetable may change when the search is provisional, and keeps it across Load more', async () => {
      vi.stubGlobal(
        'fetch',
        mockFetchByUrl({
          search: (url) =>
            url.includes('after=CURSOR1')
              ? new Response(provisionalBody(PAGE_TWO, null, true), { status: 200 })
              : new Response(provisionalBody(PAGE_ONE, 'CURSOR1', true), { status: 200 }),
        }),
      );
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);

      await clickSearch();
      await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
      expect(screen.getByText('Timetable may change')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
      await screen.findByRole('link', { name: rowName('11:40', 'WAT') });
      expect(screen.getByText('Timetable may change')).toBeInTheDocument();
    });

    it.each([false, undefined])('shows no note when provisional is %s', async (provisional) => {
      vi.stubGlobal(
        'fetch',
        mockFetchByUrl({ search: () => new Response(provisionalBody(PAGE_ONE, null, provisional), { status: 200 }) }),
      );
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);

      await clickSearch();
      await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
      expect(screen.queryByText('Timetable may change')).not.toBeInTheDocument();
    });
  });

  it('omits date from the search request when no date is picked', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&across_midnight=true'));
  });

  it('shows an accessible "no matches" option instead of hiding the listbox when a search matches nothing', () => {
    // Mantine's `Autocomplete` has no `nothingFoundMessage` prop at all
    // (unlike Select/MultiSelect) -- it hides its whole `role="listbox"`
    // dropdown outright whenever `data` is empty, leaving an open combobox
    // (`aria-expanded="true"`) with no `option`/`group` child, which fails
    // axe's `aria-required-children`. `withNoMatchPlaceholder`
    // (`lib/autocompleteNoMatch.ts`) works around the missing prop by
    // swapping in a single inert `role="option"` placeholder whenever the
    // real suggestions list is empty -- here, whenever the mocked
    // `useSuggestions` finds nothing in `TEST_STATIONS`.
    renderWithMantine(<TrainSearchForm />);
    const input = screen.getByRole('combobox', { name: 'Station' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'zzzzzz' } });

    expect(screen.getByRole('option', { name: 'No matching stations' })).toBeInTheDocument();
  });

  it('sends every optional filter it has, uppercased', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <TrainSearchForm initialStation="man" initialStopFilters={{ ...noFilters, startsAt: ['EUS'] }} />,
    );

    fireEvent.change(panelField('Earliest departure (optional)'), { target: { value: '09:00' } });
    fireEvent.change(panelField('Latest departure (optional)'), { target: { value: '12:00' } });
    await clickSearch();

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&from=09%3A00&to=12%3A00&origin=EUS&across_midnight=true',
      ),
    );
  });

  it('searches between two stations as station and stops_at', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm view="between" initialStation="MAN" initialStopsAt="OXF" />);

    fireEvent.change(panelField('Earliest departure (optional)'), { target: { value: '09:00' } });
    await clickSearch();

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&stops_at=OXF&from=09%3A00&across_midnight=true',
      ),
    );
    // No station view fields.
    expect(screen.getByRole('combobox', { name: 'From' })).toHaveValue('MAN');
    expect(screen.getByRole('combobox', { name: 'To' })).toHaveValue('OXF');
  });

  it('needs both stations before it searches between them', () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm view="between" />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'MAN' } });
    expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'WAT' } });
    expect(screen.getByRole('button', { name: 'Search' })).not.toBeDisabled();
    expect(searchCallUrls(fetchMock)).toHaveLength(0);
  });

  it('shows the arrival at the second station on each row between two stations', async () => {
    const body = JSON.stringify({
      results: [{ ...PAGE_ONE[0], destinationArrival: null, destinationName: null, stopsAtArrival: '09:47' }],
      nextCursor: null,
    });
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response(body, { status: 200 }) }));
    renderWithMantine(<TrainSearchForm view="between" initialStation="MAN" initialStopsAt="OXF" />);
    await clickSearch();

    const link = await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
    expect(link.closest('li')).toHaveTextContent(/arr.*09:47/);
    expect(
      screen.getByText(/Times are when each train leaves the first station, with its arrival at the second/),
    ).toBeInTheDocument();
  });

  describe('arrivals board', () => {
    const ARRIVALS = JSON.stringify({
      results: [
        {
          uid: 'C20001',
          scheduled: null,
          stationCrs: 'MAN',
          originCrs: 'EUS',
          originName: 'London Euston',
          destinationCrs: 'MAN',
          destinationName: 'Manchester Piccadilly',
          destinationArrival: '09:05',
          destinationArrivalDayOffset: 0,
          arrival: '09:05',
          arrivalDayOffset: 0,
          terminates: true,
        },
        {
          uid: 'C20002',
          scheduled: '09:12',
          dayOffset: 0,
          stationCrs: 'MAN',
          originCrs: 'BHM',
          originName: 'Birmingham New Street',
          destinationCrs: 'GLC',
          destinationName: 'Glasgow Central',
          destinationArrival: '12:30',
          destinationArrivalDayOffset: 0,
          arrival: '09:10',
          arrivalDayOffset: 0,
          terminates: false,
        },
      ],
      nextCursor: null,
    });

    function mockArrivals() {
      const base = mockFetchByUrl();
      return vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        if (url.startsWith('/api/trains/arrivals')) return Promise.resolve(new Response(ARRIVALS, { status: 200 }));
        return base(input);
      });
    }

    it('lists arrivals by where each train comes from, timed by its arrival', async () => {
      const fetchMock = mockArrivals();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" initialBoard="arrivals" />);

      const terminating = await screen.findByRole('link', { name: /^09:05 from London Euston\b/ });
      expect(terminating).toHaveAttribute('href', expect.stringMatching(/^\/train\/C20001\//));
      expect(terminating.closest('li')).toHaveTextContent('Terminates here');
      const calling = screen.getByRole('link', { name: /^09:10 from Birmingham New Street\b/ });
      expect(calling.closest('li')).toHaveTextContent('To Glasgow Central');
      expect(screen.getByText(/Times are when each train arrives at this station/)).toBeInTheDocument();
      expect(
        fetchMock.mock.calls.map((args: unknown[]) => String(args[0])).filter((url) => url.startsWith('/api/trains/')),
      ).toEqual(['/api/trains/arrivals?station=MAN']);
    });

    it('switches between departures and arrivals, keeping the station', async () => {
      const fetchMock = mockArrivals();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);
      await awaitMountSettled();

      fireEvent.click(screen.getByRole('radio', { name: 'Arrivals' }));
      expect(await screen.findByRole('link', { name: /^09:05 from London Euston\b/ })).toBeInTheDocument();
      fireEvent.click(screen.getByRole('radio', { name: 'Departures' }));
      expect(await screen.findByRole('link', { name: rowName('08:22', 'WAT') })).toBeInTheDocument();
      expect(screen.getByRole('combobox', { name: 'Station' })).toHaveValue('MAN');
    });

    it('names its time filters for arrivals and sends them as from and to', async () => {
      const fetchMock = mockArrivals();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" initialBoard="arrivals" />);
      await awaitMountSettled();

      fireEvent.change(panelField('Earliest arrival (optional)'), { target: { value: '09:00' } });
      await waitFor(() =>
        expect(fetchMock.mock.calls.map((args: unknown[]) => String(args[0])).at(-1)).toBe(
          '/api/trains/arrivals?station=MAN&from=09%3A00',
        ),
      );
    });
  });

  /** The four time filters are `TimeFilterInput` -- a native
   * `<input type="time">` with an explicit picker button and clear button
   * -- not the free-text `TextInput` they used to be. `TimeFilterInput`'s
   * own tests cover the control in isolation; these are the integration
   * facts only this form can prove: that all four are wired up, that their
   * affordances are distinguishable from each other, and that the swap
   * left the wire values and validation exactly as they were. */
  describe('time filters use a real time picker', () => {
    const TIME_FIELDS = [
      { label: 'Earliest departure (optional)', name: 'earliest departure' },
      { label: 'Latest departure (optional)', name: 'latest departure' },
      { label: 'Earliest arrival (optional)', name: 'earliest arrival' },
      { label: 'Latest arrival (optional)', name: 'latest arrival' },
    ];

    it('renders every time filter as a native time input with its own picker button', async () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      // `initialStopsAt` so the arrival pair (which only renders once Stops
      // at is filled in) is on screen alongside the departure pair -- all
      // four at once is the case where their button names have to differ.
      renderWithMantine(<TrainSearchForm view="between" initialStation="MAN" initialStopsAt="WAT" />);

      for (const { label, name } of TIME_FIELDS) {
        const input = panelField(label);
        expect(input).toHaveAttribute('type', 'time');
        // `step=60` is what keeps a native time input's value at "HH:MM" --
        // the exact shape `from`/`to`/`arrival_from`/`arrival_to` are parsed
        // as server-side. A seconds segment would put "HH:MM:SS" on the wire.
        expect(input).toHaveAttribute('step', '60');
        // `getByRole` throws on more than one match, so this doubles as the
        // assertion that the four buttons are uniquely named.
        expect(screen.getByRole('button', { name: `Pick ${name}` })).toBeInTheDocument();
      }
      // Lets the mount-time auto-search (`initialStation="MAN"` is a valid
      // CRS) settle before the test ends, so its state update doesn't land
      // outside `act()`.
      await awaitMountSettled();
    });

    it('keeps each time filter optional and individually clearable', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);

      fireEvent.change(panelField('Earliest departure (optional)'), {
        target: { value: '09:00' },
      });
      fireEvent.change(panelField('Latest departure (optional)'), {
        target: { value: '12:00' },
      });
      // Driven through the field's real clear button rather than a
      // synthetic empty-string change event, because that button is the
      // only way back to empty on a touch device (a native time input
      // opens a wheel picker with no keyboard to Backspace with, and
      // Mantine's own stylesheet hides the native clear control).
      fireEvent.click(screen.getByRole('button', { name: 'Clear earliest departure' }));

      // Only the cleared field's param is dropped; the other survives.
      await clickSearch();

      await waitFor(() =>
        expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&to=12%3A00&across_midnight=true'),
      );
    });

    it('offers a clear button only on the time filters that have a value', async () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);

      expect(screen.queryByRole('button', { name: 'Clear earliest departure' })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Clear latest departure' })).not.toBeInTheDocument();

      fireEvent.change(panelField('Earliest departure (optional)'), {
        target: { value: '09:00' },
      });

      expect(screen.getByRole('button', { name: 'Clear earliest departure' })).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Clear latest departure' })).not.toBeInTheDocument();

      await awaitMountSettled();
    });

    it('still flags a time the wire format does not accept', async () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderFullForm();
      // `canSearch` is gated on `!searching` too, so the disabled-button
      // assertion below would otherwise be racing this form's own
      // mount-time auto-search, not just the invalid time under test.
      await awaitMountSettled();

      // `"09:00:30"` is a VALID HTML time string, so a `type="time"`
      // input's own value sanitization lets it straight through to
      // `onChange` (jsdom does too -- verified, unlike outright garbage
      // such as `"25:99"`, which sanitizes to `""` below). It is still not
      // a value this API accepts: `from`/`to`/`arrival_from`/`arrival_to`
      // are parsed as bare `"HH:MM"`. So the component's own
      // `TIME_PATTERN` check is not dead weight after the swap to a picker
      // -- this is the case it still catches.
      fireEvent.change(panelField('Earliest departure (optional)'), {
        target: { value: '09:00:30' },
      });

      expect(screen.getByText('Must be a time like 09:00')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    });

    it('refuses a nonsense time outright instead of putting it on the wire', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);

      // The upgrade from a free-text box: `"25:99"` is not a valid HTML
      // time string, so the control never adopts it at all (it sanitizes
      // to `""`). The old `TextInput` accepted the keystrokes and only
      // *then* showed an error; now there is nothing to error about,
      // and `from` is simply absent -- never sent as a malformed value.
      fireEvent.change(panelField('Earliest departure (optional)'), {
        target: { value: '25:99' },
      });
      expect(panelField('Earliest departure (optional)')).toHaveValue('');

      await clickSearch();

      await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&across_midnight=true'));
    });

    it('refuses to search on a half-entered time rather than quietly dropping the filter', async () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderFullForm();
      // `canSearch` is gated on `!searching` too, so the disabled-button
      // assertion below would otherwise be racing this form's own
      // mount-time auto-search, not just the incomplete time under test.
      await awaitMountSettled();

      const input = panelField('Earliest departure (optional)');
      // A real browser reports a half-entered time ("09:--") as `''` with
      // `validity.badInput` set; jsdom models neither, so the flag is
      // forced. Without the wiring this test covers, `''` is
      // indistinguishable from an untouched field and the search would run
      // with no `from` at all.
      Object.defineProperty(input, 'validity', { configurable: true, get: () => ({ badInput: true }) });
      fireEvent.blur(input);

      expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    });

    it('stops refusing once a half-entered time is cleared away', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);

      const input = panelField('Earliest departure (optional)');
      Object.defineProperty(input, 'validity', { configurable: true, get: () => ({ badInput: true }) });
      fireEvent.blur(input);

      Object.defineProperty(input, 'validity', { configurable: true, get: () => ({ badInput: false }) });
      fireEvent.click(screen.getByRole('button', { name: 'Clear earliest departure' }));

      await clickSearch();
      await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&across_midnight=true'));
    });

    it('refuses to search between two stations on a half-entered arrival time', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<TrainSearchForm view="between" />);
      fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'MAN' } });
      fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'WAT' } });
      expect(screen.getByRole('button', { name: 'Search' })).not.toBeDisabled();

      const arrival = panelField('Earliest arrival (optional)');
      Object.defineProperty(arrival, 'validity', { configurable: true, get: () => ({ badInput: true }) });
      fireEvent.blur(arrival);
      expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    });

    it('does not let the picker or clear button submit the form', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);
      await awaitMountSettled();
      // A baseline, not zero: `initialStation="MAN"` is a valid CRS, so the
      // mount-time auto-search (§3.3) already made one call of its own by
      // this point. What this test actually guards is that NEITHER button
      // below adds another one.
      const callsBeforeInteraction = searchCallUrls(fetchMock).length;

      // Every field sits inside `<Stack component="form">`, so a
      // right-section button defaulting to `type="submit"` would fire a
      // whole search on each click.
      fireEvent.change(panelField('Earliest departure (optional)'), {
        target: { value: '09:00' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Pick earliest departure' }));
      fireEvent.click(screen.getByRole('button', { name: 'Clear earliest departure' }));

      expect(searchCallUrls(fetchMock)).toHaveLength(callsBeforeInteraction);
    });

    it('blocks the search on an unacceptable ARRIVAL time too, not just a departure one', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<TrainSearchForm view="between" />);
      fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'MAN' } });
      fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'WAT' } });

      // The arrival pair is conditionally rendered, so its contribution to
      // `canSearch` is easy to lose without noticing -- pinned separately
      // from the departure pair's for that reason.
      fireEvent.change(panelField('Latest arrival (optional)'), {
        target: { value: '09:30:15' },
      });

      expect(screen.getByText('Must be a time like 09:00')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    });
  });

  it('renders one row per result, with time, destination, origin and status', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    const first = await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
    expect(first.closest('li')).toHaveTextContent('From EUS');
    // No live state from this backend yet: every row reads "Scheduled".
    expect(first.closest('li')).toHaveTextContent('Scheduled');
    expect(screen.getByRole('link', { name: rowName('10:05', 'WAT') }).closest('li')).toHaveTextContent('From CRE');
  });

  it("names each row's operator from operatorNames on its details line", async () => {
    const rows = [
      { uid: 'C10001', scheduled: '08:22', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT', operator: 'GW' },
    ];
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response(searchBody(rows), { status: 200 }) }));
    renderWithMantine(<TrainSearchForm initialStation="MAN" operatorNames={{ GW: 'Great Western Railway' }} />);

    await clickSearch();

    const row = await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
    expect(row.closest('li')).toHaveTextContent('From EUS · Great Western Railway (GW)');
  });

  it("shows each row's headcode, labelled, on its details line", async () => {
    const rows = [
      {
        uid: 'C10001',
        scheduled: '08:22',
        stationCrs: 'MAN',
        originCrs: 'EUS',
        destinationCrs: 'WAT',
        identity: '1A23',
      },
      {
        uid: 'C10002',
        scheduled: '10:05',
        stationCrs: 'MAN',
        originCrs: 'CRE',
        destinationCrs: 'WAT',
        headcode: '2B45',
      },
      { uid: 'C10003', scheduled: '11:40', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT', identity: null },
    ];
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response(searchBody(rows), { status: 200 }) }));
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    const first = await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
    expect(first.closest('li')).toHaveTextContent('From EUS · Headcode 1A23');
    // An older payload's `headcode` is the fallback.
    expect(screen.getByRole('link', { name: rowName('10:05', 'WAT') }).closest('li')).toHaveTextContent(
      'From CRE · Headcode 2B45',
    );
    expect(screen.getByRole('link', { name: rowName('11:40', 'WAT') }).closest('li')).not.toHaveTextContent('Headcode');
  });

  it('shows no operator on the details line without operatorNames', async () => {
    const rows = [
      { uid: 'C10001', scheduled: '08:22', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT', operator: 'GW' },
    ];
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response(searchBody(rows), { status: 200 }) }));
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    const row = await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
    expect(row.closest('li')).toHaveTextContent('From EUS');
    expect(row.closest('li')).not.toHaveTextContent('GW');
  });

  // Regression: `destinationCrs` used to be the ONLY thing this row could
  // show for a destination -- the backend now resolves a `destinationName`
  // alongside it (`render::calling_point_departure_json`), and this row
  // must prefer the name when one resolved, falling back to the bare code
  // otherwise (a row with no name, or a response from a backend build
  // predating this field entirely, must render exactly as it did before).
  it('prefers destinationName over the bare destinationCrs when one resolves', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: () =>
          new Response(
            searchBody([
              {
                uid: 'C10001',
                scheduled: '08:22',
                stationCrs: 'MAN',
                originCrs: 'EUS',
                destinationCrs: 'WAT',
                destinationName: 'London Waterloo',
              },
              { uid: 'C10002', scheduled: '10:05', stationCrs: 'MAN', originCrs: 'CRE', destinationCrs: 'WAT' },
            ]),
            { status: 200 },
          ),
      }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    expect(await screen.findByRole('link', { name: rowName('08:22', 'London Waterloo') })).toBeInTheDocument();
    // The second row has no destinationName in this response -- falls back
    // to the bare code exactly as before this field existed.
    expect(screen.getByRole('link', { name: rowName('10:05', 'WAT') })).toBeInTheDocument();
  });

  // Regression guard for the results list being hard-clipped at a fixed
  // height. The rows used to sit inside a `<ScrollArea mah={420}
  // offsetScrollbars>`, whose root is `overflow: hidden` while its viewport
  // is `height: 100%`. With only `mah` on the root, the root's own `height`
  // stays `auto`, so that `100%` resolves to `auto` too (CSS 2.1 §10.5: a
  // percentage height against a content-sized containing block computes to
  // `auto`) -- the viewport grows to its full content height and therefore
  // never overflows *itself*, so it never scrolls, while the root clamps to
  // the cap and clips everything past it with `overflow: hidden`. No
  // pointer, wheel or scrollbar gesture could reach a row past 420px, and
  // each "Load more" appended rows straight into the clipped region.
  // Nothing hinted anything had been cut off either: Mantine hides the
  // native scrollbar (`scrollbar-width: none`) and draws its own, sized
  // from `scrollHeight` vs `clientHeight` -- equal here -- so it never
  // appeared.
  //
  // `ScrollArea.Autosize` IS the Mantine component that supports a max
  // height (it wraps the root in a `display: flex` / `flex: 1` / `overflow:
  // hidden` chain, which is what makes the root's height definite). It is
  // still not what's used: letting the page scroll is what
  // `StationTimetable.tsx` (the other paginated "Load more" list over these
  // same CIF rows) and `IncidentSearchForm.tsx` (which this component's
  // results section mirrors) both already do. A nested scroller buys
  // nothing here -- there are no sticky controls above the list to keep on
  // screen -- while costing real usability on touch, where it steals the
  // page's own scroll gesture.
  it('renders the results list in the page flow, with no fixed-height or scroll-container ancestor', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();
    await screen.findByRole('link', { name: rowName('08:22', 'WAT') });

    const list = document.querySelector('[data-train-results]');
    expect(list).not.toBeNull();
    expect(list?.tagName).toBe('UL');
    // The compact bar is not a form: walk up to the rendered root instead.
    const form = document.body.firstElementChild as HTMLElement;

    // Walk the list itself plus every ancestor up to (and including) the
    // form -- a clip anywhere on that chain hides rows just as effectively
    // as one on the list. (Above the form is this component's caller, which
    // a unit test can't see.)
    for (
      let node: HTMLElement | null = list as HTMLElement;
      node !== null;
      node = node === form ? null : node.parentElement
    ) {
      // Mantine's own scroll viewport, whatever set it up.
      expect(node.hasAttribute('data-scrollarea-viewport')).toBe(false);
      // Mantine resolves a non-responsive `h`/`mah` style prop straight into
      // an inline `height`/`max-height` (`parse-style-props.mjs`), so
      // reading those back off `style` is enough -- no computed style, no
      // layout, which is just as well under jsdom.
      expect(node.style.maxHeight).toBe('');
      expect(node.style.height).toBe('');
      // Only catches a hand-written inline clip -- Mantine's own
      // `overflow: hidden` arrives via the `.m_d57069b5` class, which the
      // `data-scrollarea-viewport` check above is what actually covers.
      // Known gap, accepted: a RESPONSIVE `mah={{ base: 420 }}` compiles to
      // a generated stylesheet rule rather than an inline style, as would a
      // clip arriving via a CSS module or a global class, and neither would
      // be seen here. The `data-scrollarea-viewport` check still catches
      // every `ScrollArea`-shaped reintroduction, which is the realistic
      // one.
      expect(node.style.overflow).not.toBe('hidden');
      expect(node.style.overflowY).not.toBe('hidden');
    }
  });

  it('keeps every "Load more" page in the one in-flow list', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: (url) =>
          url.includes('after=CURSOR1')
            ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
            : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
      }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();
    await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await screen.findByRole('link', { name: rowName('11:40', 'WAT') });

    // Page 2's rows must land in the SAME in-flow list as page 1's, so the
    // page's own scrollbar reaches them -- under the removed `ScrollArea`
    // each "Load more" appended straight into the clipped region.
    const list = document.querySelector('[data-train-results]') as HTMLElement;
    for (const [time, destination] of [
      ['08:22', 'WAT'],
      ['10:05', 'WAT'],
      ['11:40', 'WAT'],
    ] as const) {
      expect(list.contains(screen.getByRole('link', { name: rowName(time, destination) }))).toBe(true);
    }
  });

  // A row has two controls: the destination link to the train's page and
  // the "Track this train" button in the row's actions slot. The button
  // must never sit inside the link (nested interactive content), and both
  // belong to the same row.
  it("puts the ticket flow's track button beside the row's link, never inside it", async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" attachTicketId={7} />);

    await clickSearch();
    const link = await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
    const row = link.closest('li') as HTMLElement;
    const button = within(row).getByRole('button', { name: 'Track this train' });
    expect(button.closest('a')).toBeNull();
    expect(link).not.toContainElement(button);
    expect(within(row).getAllByRole('link')).toHaveLength(1);
  });

  it('leaves the origin out when it is unknown, rather than a "?"', async () => {
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

    await clickSearch();

    const link = await screen.findByRole('link', { name: rowName('09:00', 'WAT') });
    expect(link.closest('li')).not.toHaveTextContent('From');
    expect(link.closest('li')).not.toHaveTextContent('?');
  });

  it("links each row to the public train page for London's today", async () => {
    // FE-4: 23:30 UTC on 15 July is 00:30 on 16 July in London (BST).
    vi.useFakeTimers({ toFake: ['Date'], shouldAdvanceTime: true });
    vi.setSystemTime(new Date('2026-07-15T23:30:00Z'));
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    const link = await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
    expect(link).toHaveAttribute('href', '/train/C10001/2026-07-16');
  });

  it('makes each row just its link to the train, with no tracking action', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    const link = await screen.findByRole('link', { name: rowName('08:22', 'WAT') });
    expect(link.tagName).toBe('A');
    expect(link.closest('li')).toBe(link.parentElement);
    expect(screen.queryByRole('button', { name: /^Track this/ })).not.toBeInTheDocument();
  });

  it('renders a Track this train action on every row in the ticket flow', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" attachTicketId={7} />);

    await clickSearch();

    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    expect(buttons).toHaveLength(2);
  });

  it("passes attachTicketId through, so the row's track action attaches the ticket", async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" attachTicketId={7} />);

    await clickSearch();
    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    fireEvent.click(buttons[0]!);

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/tickets/7/attach',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trackingId: 42 }) }),
      ),
    );
  });

  it('distinguishes "nothing published for today" from "no matches"', async () => {
    const fetchMock = mockFetchByUrl({ search: () => new Response('not found', { status: 404 }) });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="ZZZ" />);

    await clickSearch();

    expect(await screen.findByText(/Today's scheduled timetable data isn't available yet/)).toBeInTheDocument();
    // One request: the compact bar searches on mount and on change only.
    expect(searchCallUrls(fetchMock)).toHaveLength(1);
  });

  it('says so when the search succeeds but matches nothing', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response(searchBody([]), { status: 200 }) }));
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    expect(await screen.findByText('No scheduled trains match those filters right now.')).toBeInTheDocument();
  });

  it('shows an error state on a 500', async () => {
    const fetchMock = mockFetchByUrl({ search: () => new Response('boom', { status: 500 }) });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    expect(await screen.findByText("Couldn't search for trains right now. Try again.")).toBeInTheDocument();
    expect(searchCallUrls(fetchMock)).toHaveLength(1);
  });

  it('labels the results as scheduled timetable data, not live status', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    expect(
      await screen.findByText(/Times are when each train leaves this station, from the scheduled timetable/),
    ).toBeInTheDocument();
  });

  it('offers the manual /track fallback, carrying any ticketId through', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm attachTicketId={7} />);

    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute('href', '/track?ticketId=7');
  });

  it('offers no tracking fallback outside the ticket flow', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.queryByRole('link', { name: 'Track it manually' })).not.toBeInTheDocument();
  });

  describe('operator filter', () => {
    const OPERATORS = { SW: 'South Western Railway', GW: 'Great Western Railway' };

    it('offers the operators by name and searches any of the ones picked', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" operatorNames={OPERATORS} />);
      await awaitMountSettled();

      fireEvent.click(panelCombobox('Operator (optional)'));
      const options = screen.getAllByRole('option').map((option) => option.textContent);
      // By name, code first, as on /incidents.
      expect(options).toEqual(['GW · Great Western Railway', 'SW · South Western Railway']);
      fireEvent.click(screen.getByRole('option', { name: 'GW · Great Western Railway' }));
      fireEvent.click(screen.getByRole('option', { name: 'SW · South Western Railway' }));

      await waitFor(() =>
        expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&operator=GW%2CSW&across_midnight=true'),
      );
      expect(screen.getByTestId('list-filter-count')).toHaveTextContent('1');
    });

    it('reads ?operator= tolerantly, dropping codes it does not know', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(
        <TrainSearchForm initialStation="MAN" initialOperator=" sw,XX,,gw,SW,toolong" operatorNames={OPERATORS} />,
      );
      await awaitMountSettled();

      expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&operator=SW%2CGW&across_midnight=true');
    });

    it('clears with the other filters', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" initialOperator="SW" operatorNames={OPERATORS} />);
      await awaitMountSettled();

      openFiltersIfCompact();
      fireEvent.click(screen.getByRole('button', { name: /^Clear filters/ }));

      await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&across_midnight=true'));
    });
  });

  describe('times after midnight', () => {
    /** The "Next day" box under each time field, in field order. */
    function nextDayBoxes(): HTMLElement[] {
      return screen.getAllByRole('checkbox', { name: /^Next day, Sat 24 Oct/ });
    }

    it('shows a Next day box, naming the next date, once a time is set', async () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<TrainSearchForm initialStation="MAN" initialDate="2026-10-23" />);
      await awaitMountSettled();

      openFiltersIfCompact();
      expect(screen.queryByRole('checkbox', { name: /^Next day/ })).not.toBeInTheDocument();
      fireEvent.change(panelField('Earliest departure (optional)'), { target: { value: '00:30' } });
      expect(nextDayBoxes()).toHaveLength(1);
    });

    it('sends an earliest time after midnight as from_day_offset=1', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" initialDate="2026-10-23" />);
      await awaitMountSettled();

      fireEvent.change(panelField('Earliest departure (optional)'), { target: { value: '00:30' } });
      fireEvent.click(nextDayBoxes()[0]!);

      await waitFor(() =>
        expect(searchCallUrl(fetchMock)).toBe(
          '/api/trains/search?station=MAN&date=2026-10-23&from=00%3A30&from_day_offset=1',
        ),
      );
    });

    it('ticks and locks the latest time after midnight when it is earlier than the earliest', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" initialDate="2026-10-23" />);
      await awaitMountSettled();

      fireEvent.change(panelField('Earliest departure (optional)'), { target: { value: '22:00' } });
      fireEvent.change(panelField('Latest departure (optional)'), { target: { value: '01:30' } });

      const [fromBox, toBox] = nextDayBoxes();
      expect(fromBox).not.toBeChecked();
      expect(toBox).toBeChecked();
      expect(toBox).toBeDisabled();
      expect(screen.getByText('Earlier than the earliest departure, so after midnight.')).toBeInTheDocument();
      // The API reads "to before from" the same way, so the box the visitor
      // did not tick stays out of the query.
      await waitFor(() =>
        expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&date=2026-10-23&from=22%3A00&to=01%3A30'),
      );
    });

    it('restores a ticked box from the URL', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(
        <TrainSearchForm initialStation="MAN" initialDate="2026-10-23" initialTo="06:00" initialToDayOffset="1" />,
      );
      await awaitMountSettled();

      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&date=2026-10-23&to=06%3A00&to_day_offset=1',
      );
      openFiltersIfCompact();
      expect(nextDayBoxes()[0]).toBeChecked();
    });

    it('lists the trains leaving after midnight under the next date, marked +1', async () => {
      const rows = [
        { uid: 'C20001', scheduled: '23:40', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT', dayOffset: 0 },
        { uid: 'C20002', scheduled: '00:30', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'OXF', dayOffset: 1 },
      ];
      vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response(searchBody(rows), { status: 200 }) }));
      renderWithMantine(<TrainSearchForm initialStation="MAN" initialDate="2026-10-23" />);
      await awaitMountSettled();

      const evening = screen.getByRole('list', { name: 'Trains' });
      expect(within(evening).getByRole('link', { name: rowName('23:40', 'WAT') })).toBeInTheDocument();
      const afterMidnight = screen.getByRole('list', { name: 'After midnight, Sat 24 Oct' });
      const late = within(afterMidnight).getByRole('link', { name: /^00:30 \(next day\) to OXF/ });
      expect(late.closest('li')).toHaveTextContent('+1');
      // Still the searched date's train.
      expect(late).toHaveAttribute('href', expect.stringContaining('2026-10-23'));
    });

    it("lists yesterday's trains still running first, under and labelled with their service date", async () => {
      // 23:30 UTC on Fri 23 Oct is 00:30 on Sat 24 Oct in London (BST).
      vi.useFakeTimers({ toFake: ['Date'], shouldAdvanceTime: true });
      vi.setSystemTime(new Date('2026-10-23T23:30:00Z'));
      const rows = [
        {
          uid: 'C30001',
          scheduled: '00:40',
          stationCrs: 'MAN',
          originCrs: 'EUS',
          destinationCrs: 'OXF',
          dayOffset: 1,
          serviceDate: '2026-10-23',
        },
        {
          uid: 'C30002',
          scheduled: '00:45',
          stationCrs: 'MAN',
          originCrs: 'EUS',
          destinationCrs: 'WAT',
          dayOffset: 0,
          serviceDate: '2026-10-24',
        },
      ];
      const fetchMock = mockFetchByUrl({ search: () => new Response(searchBody(rows), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);
      await clickSearch();

      // No date picked: yesterday's late trains are asked for too.
      expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&across_midnight=true');
      const previous = await screen.findByRole('list', { name: 'Still running from Fri 23 Oct' });
      const late = within(previous).getByRole('link', { name: /^00:40 \(next day\) to OXF/ });
      expect(late.closest('li')).toHaveTextContent('Fri 23 Oct service');
      expect(late.closest('li')).toHaveTextContent('+1');
      // Its own date's train page.
      expect(late).toHaveAttribute('href', '/train/C30001/2026-10-23');

      const today = screen.getByRole('list', { name: 'Trains' });
      const own = within(today).getByRole('link', { name: rowName('00:45', 'WAT') });
      expect(own).toHaveAttribute('href', '/train/C30002/2026-10-24');
      expect(own.closest('li')).not.toHaveTextContent('service');
    });

    it('keeps a picked date to its own trains', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" initialDate="2026-10-23" />);
      await awaitMountSettled();

      expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&date=2026-10-23');
      expect(screen.queryByRole('list', { name: /^Still running/ })).not.toBeInTheDocument();
    });

    it('offers an operator the timetable carries with no name, by its code', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(
        <TrainSearchForm
          initialStation="MAN"
          initialOperator="TW,ZZ"
          operatorNames={{ SW: 'South Western Railway' }}
          unnamedOperatorCodes={['TW']}
        />,
      );
      // `TW` is kept from the link (a known code); `ZZ` is not.
      return waitFor(() =>
        expect(searchCallUrl(vi.mocked(fetch) as ReturnType<typeof vi.fn>)).toBe(
          '/api/trains/search?station=MAN&operator=TW&across_midnight=true',
        ),
      );
    });
  });

  it('does not offer Load more when the response has no nextCursor', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    expect(await screen.findByRole('link', { name: rowName('08:22', 'WAT') })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
    // The button doesn't just vanish -- the list says it is complete.
    expect(
      screen.getByText("You've reached the end. No more scheduled trains match those filters."),
    ).toBeInTheDocument();
  });

  it('says the end has been reached once the last page is in, rather than just dropping the button', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: (url) =>
          url.includes('after=CURSOR1')
            ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
            : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
      }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();
    // While a next page exists, the end must not be claimed.
    expect(await screen.findByRole('button', { name: 'Load more' })).toBeInTheDocument();
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(
      await screen.findByText("You've reached the end. No more scheduled trains match those filters."),
    ).toBeInTheDocument();
  });

  it('does not claim the end of results when a Load more page fails -- it reports the failure and keeps the retry', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: (url) =>
          url.includes('after=CURSOR1')
            ? new Response('boom', { status: 500 })
            : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
      }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText("Couldn't load more results. Try again.")).toBeInTheDocument();
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Load more' })).toBeEnabled();
    // A failed next page must not wipe out the page already on screen, nor
    // escalate to the whole-search error state.
    expect(screen.getByRole('link', { name: rowName('08:22', 'WAT') })).toBeInTheDocument();
    expect(screen.queryByText("Couldn't search for trains right now. Try again.")).not.toBeInTheDocument();
  });

  it('clears a previous Load more failure when a fresh search is run', async () => {
    let failNextPage = true;
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: (url) =>
          url.includes('after=CURSOR1') && failNextPage
            ? new Response('boom', { status: 500 })
            : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
      }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    await screen.findByText("Couldn't load more results. Try again.");

    failNextPage = false;
    changeDate('2026-09-16');
    await clickSearch();

    await waitFor(() => expect(screen.queryByText("Couldn't load more results. Try again.")).not.toBeInTheDocument());
  });

  it('shows neither Load more nor the end-of-results line when the search matched nothing', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response(searchBody([]), { status: 200 }) }));
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();

    await screen.findByText('No scheduled trains match those filters right now.');
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
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

    await clickSearch();

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

    await clickSearch();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByRole('link', { name: rowName('11:40', 'WAT') })).toBeInTheDocument();
    expect(
      screen.getByRole('link', { name: rowName('08:22', 'WAT') }),
      'page 1 must still be on screen -- Load more appends, it does not replace',
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: rowName('10:05', 'WAT') })).toBeInTheDocument();

    const urls = searchCallUrls(fetchMock);
    // Index 0 is the mount-time search (`initialStation="MAN"` is a valid
    // CRS); index 1 is "Load more".
    expect(urls).toHaveLength(2);
    expect(urls[0]).toBe('/api/trains/search?station=MAN&across_midnight=true');
    expect(urls[1]).toBe('/api/trains/search?station=MAN&after=CURSOR1&across_midnight=true');

    await waitFor(() => expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument());
  });

  it('uses the NEW cursor on a second Load more, not the first one again', async () => {
    const fetchMock = mockFetchByUrl({
      search: (url) => {
        if (url.includes('after=CURSOR2')) return new Response(searchBody(PAGE_THREE, null), { status: 200 });
        if (url.includes('after=CURSOR1')) return new Response(searchBody(PAGE_TWO, 'CURSOR2'), { status: 200 });
        return new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 });
      },
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    await clickSearch();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    expect(await screen.findByRole('link', { name: rowName('11:40', 'WAT') })).toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByRole('link', { name: rowName('13:15', 'WAT') })).toBeInTheDocument();

    const urls = searchCallUrls(fetchMock);
    // Index 0 is the mount-time search; indices 1/2 the two "Load more"
    // presses.
    expect(urls).toHaveLength(3);
    expect(urls[1]).toBe('/api/trains/search?station=MAN&after=CURSOR1&across_midnight=true');
    expect(urls[2], 'the second Load more must use the cursor from the SECOND response').toBe(
      '/api/trains/search?station=MAN&after=CURSOR2&across_midnight=true',
    );
    expect(screen.getAllByRole('link', { name: rowName('11:40', 'WAT') })).toHaveLength(1);
  });

  it('keeps the original filters on a Load more request', async () => {
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <TrainSearchForm initialStation="man" initialStopFilters={{ ...noFilters, startsAt: ['EUS'] }} />,
    );

    fireEvent.change(panelField('Earliest departure (optional)'), { target: { value: '09:00' } });
    fireEvent.change(panelField('Latest departure (optional)'), { target: { value: '12:00' } });
    await clickSearch();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    await waitFor(() => expect(searchCallUrl(fetchMock)).toContain('after=CURSOR1'));
    // The last call is "Load more", continuing the search with the time
    // filters (the changes land before or after the mount search; either
    // way the rows on screen are from the search that has them).
    expect(searchCallUrl(fetchMock)).toBe(
      '/api/trains/search?station=MAN&from=09%3A00&to=12%3A00&origin=EUS&after=CURSOR1&across_midnight=true',
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

    await clickSearch();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    expect(await screen.findByRole('link', { name: rowName('11:40', 'WAT') })).toBeInTheDocument();

    changeDate('2026-09-16');
    await clickSearch();

    await waitFor(() => expect(screen.queryByRole('link', { name: rowName('11:40', 'WAT') })).not.toBeInTheDocument());
    expect(screen.getByRole('link', { name: rowName('08:22', 'WAT') })).toBeInTheDocument();
  });

  it('discards a Load more page that lands after a fresh search has already replaced the results', async () => {
    // Search is not disabled while a page is in flight, so this ordering is
    // reachable: page 2 of the OLD search resolves last. Its rows and its
    // cursor belong to a result set that is no longer on screen and must not
    // be merged into the new one. (IncidentSearchForm.test.tsx pins the same
    // guard on the same shape of bug.)
    let resolvePageTwo!: (response: Response) => void;
    const pageTwo = new Promise<Response>((resolve) => {
      resolvePageTwo = resolve;
    });
    let searchCalls = 0;
    const passThrough = mockFetchByUrl();
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      if (!url.startsWith('/api/trains/search')) return passThrough(input);
      searchCalls += 1;
      // Call 1 is this form's own mount-time auto-search (`initialStation=
      // "MAN"` is a valid CRS) -- it stands in for "search 1" below with no
      // explicit click needed. Call 2 is page 2 of that search, held open;
      // call 3 is search 2 (the explicit re-search).
      if (searchCalls === 1) {
        return Promise.resolve(new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }));
      }
      if (searchCalls === 2) return pageTwo;
      return Promise.resolve(new Response(searchBody(PAGE_THREE, null), { status: 200 }));
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(await screen.findByRole('button', { name: 'Load more' })); // page 2 of search 1
    changeDate('2026-09-16'); // search 2
    await clickSearch();
    await screen.findByRole('link', { name: rowName('13:15', 'WAT') });

    resolvePageTwo(new Response(searchBody(PAGE_TWO, 'CURSOR2'), { status: 200 }));

    await waitFor(() =>
      expect(
        screen.getByText("You've reached the end. No more scheduled trains match those filters."),
      ).toBeInTheDocument(),
    );
    expect(screen.queryByRole('link', { name: rowName('11:40', 'WAT') })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('sends the second station uppercased when typed in directly (not just via a suggestion pick)', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm view="between" initialStation="MAN" initialStopsAt="WAT" />);
    await awaitMountSettled();

    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), {
      target: { value: 'rdg' },
    });
    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&stops_at=RDG&across_midnight=true'),
    );
  });

  it('has no second station or arrival-time filter on a station board', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" initialStopsAt="WAT" />);
    await awaitMountSettled();

    openFiltersIfCompact();
    expect(screen.queryByLabelText('Earliest arrival (optional)')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Latest arrival (optional)')).not.toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'To' })).not.toBeInTheDocument();
  });

  it('renders the arrival-time filter between two stations', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm view="between" initialStation="MAN" initialStopsAt="WAT" />);

    expect(panelField('Earliest arrival (optional)')).toBeInTheDocument();
    expect(panelField('Latest arrival (optional)')).toBeInTheDocument();

    await awaitMountSettled();
  });

  it('sends arrival_from/arrival_to between two stations', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm view="between" initialStation="man" initialStopsAt="wat" />);

    fireEvent.change(panelField('Earliest arrival (optional)'), { target: { value: '09:00' } });
    fireEvent.change(panelField('Latest arrival (optional)'), { target: { value: '09:30' } });
    await clickSearch();

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&stops_at=WAT&arrival_from=09%3A00&arrival_to=09%3A30&across_midnight=true',
      ),
    );
  });

  it('stops searching between two stations once the second is cleared', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm view="between" initialStation="MAN" initialStopsAt="RDG" />);
    await awaitMountSettled();
    const calls = searchCallUrls(fetchMock).length;

    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: '' } });
    fireEvent.change(panelField('Earliest arrival (optional)'), { target: { value: '09:00' } });
    await new Promise((resolve) => setTimeout(resolve, 500));

    expect(searchCallUrls(fetchMock)).toHaveLength(calls);
  });

  // docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md
  // -- on `/trains`, a search used to live only in this form's own
  // `useState`, never written back to the URL, so following a result to
  // `/train/[uid]/[date]` and pressing Back lost the search entirely (both
  // the form fields and the results), because Back re-delivers the
  // ORIGINAL, never-updated `/trains` URL to a brand-new component
  // instance. These three tests cover the fix's three moving parts: writing
  // the search to the URL (`router.replace`), reading a fuller set of
  // filters back OUT of the URL (the four time-range `initialX` props), and
  // re-running the search automatically on mount so a restored URL restores
  // the RESULTS too, not just the form fields.
  describe('URL state persistence on search (train-search-state-persistence-design)', () => {
    it('replaces the URL with the query string it just searched, not pushes a new history entry', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(
        <TrainSearchForm initialStation="MAN" initialStopFilters={{ ...noFilters, startsAt: ['EUS'] }} />,
      );

      // Mount's own search does not write to the URL (it came from it).
      window.history.replaceState(null, '', '/trains?station=MAN&origin=EUS&ticketId=7');
      const replaceState = vi.spyOn(window.history, 'replaceState');
      await awaitMountSettled();

      fireEvent.change(panelField('Earliest departure (optional)'), { target: { value: '09:00' } });
      await clickSearch();

      // `history.replaceState`, never the router: `router.replace` re-ran
      // the page's server render (refetching the operator list and search
      // dates) on every search. Replace, not push, so /trains stays one
      // history entry. Params the form doesn't own (`ticketId`) are kept.
      // The URL says it in the timetable's words: `time`, not the API's `from`.
      await waitFor(() => expect(window.location.search).toBe('?station=MAN&origin=EUS&ticketId=7&time=09%3A00'));
      expect(replaceState).toHaveBeenCalledTimes(1);
      expect(replaceMock).not.toHaveBeenCalled();
      expect(pushMock).not.toHaveBeenCalled();
      replaceState.mockRestore();
      window.history.replaceState(null, '', '/');
    });

    it('writes the between-stations view and the arrivals board to the URL in their own words', async () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      window.history.replaceState(null, '', '/trains?from=MAN&to=WAT');
      const { unmount } = renderWithMantine(
        <TrainSearchForm view="between" initialStation="MAN" initialStopsAt="WAT" />,
      );
      await awaitMountSettled();
      fireEvent.change(panelField('Latest departure (optional)'), { target: { value: '10:00' } });
      await waitFor(() => expect(window.location.search).toBe('?from=MAN&to=WAT&until=10%3A00'));
      unmount();

      window.history.replaceState(null, '', '/trains?station=MAN');
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);
      await awaitMountSettled();
      fireEvent.click(screen.getByRole('radio', { name: 'Arrivals' }));
      await waitFor(() => expect(window.location.search).toBe('?station=MAN&board=arrivals'));
      window.history.replaceState(null, '', '/');
    });

    it('pre-fills every field -- including all four time-range filters -- from initial props', async () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(
        <TrainSearchForm
          view="between"
          initialStation="MAN"
          initialStopsAt="WAT"
          initialDate="2026-09-16"
          initialFrom="09:00"
          initialTo="12:00"
          initialArrivalFrom="10:00"
          initialArrivalTo="13:00"
        />,
      );
      await awaitMountSettled();

      expect(screen.getByRole('combobox', { name: 'From' })).toHaveValue('MAN');
      expect(screen.getByRole('combobox', { name: 'To' })).toHaveValue('WAT');
      expect(panelField('Date (optional)')).toHaveValue('2026-09-16');
      expect(panelField('Earliest departure (optional)')).toHaveValue('09:00');
      expect(panelField('Latest departure (optional)')).toHaveValue('12:00');
      expect(panelField('Earliest arrival (optional)')).toHaveValue('10:00');
      expect(panelField('Latest arrival (optional)')).toHaveValue('13:00');
    });

    it('auto-runs the search on mount when the initial station is valid, with no click at all', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(
        <TrainSearchForm
          initialStation="MAN"
          initialStopFilters={{ ...noFilters, startsAt: ['EUS'] }}
          initialFrom="09:00"
        />,
      );

      await waitFor(() => expect(searchCallUrls(fetchMock)).toHaveLength(1));
      expect(searchCallUrls(fetchMock)[0]).toBe(
        '/api/trains/search?station=MAN&from=09%3A00&origin=EUS&across_midnight=true',
      );
      expect(await screen.findByRole('link', { name: rowName('08:22', 'WAT') })).toBeInTheDocument();
      // The mount effect itself must not touch the URL -- only an explicit
      // Search press does (see the `router.replace` test above).
      expect(replaceMock).not.toHaveBeenCalled();
    });

    it('does not auto-run a search on mount when there is no initial station', () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialFrom="09:00" />);

      expect(searchCallUrls(fetchMock)).toHaveLength(0);
    });
  });
});

describe('TrainSearchForm: buses and ferries', () => {
  it('a bus row has its badge, reads "Timetable only" and, in the ticket flow, a "Track this bus" button', async () => {
    const body = JSON.stringify({
      results: [
        {
          uid: 'C30818',
          scheduled: '08:22',
          stationCrs: 'MAN',
          originCrs: 'MAN',
          destinationCrs: 'WAT',
          destinationArrival: null,
          destinationName: null,
          serviceMode: 'bus',
          liveTracking: false,
        },
        { ...PAGE_ONE[1], destinationArrival: null, destinationName: null, serviceMode: 'train', liveTracking: true },
      ],
      nextCursor: null,
    });
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response(body, { status: 200 }) }));
    renderWithMantine(<TrainSearchForm initialStation="MAN" attachTicketId={7} />);
    await clickSearch();

    expect(await screen.findByText('Bus service')).toBeInTheDocument();
    const bus = screen.getByRole('link', { name: rowName('08:22', 'WAT') });
    expect(bus.getAttribute('href')).toMatch(/^\/train\/C30818\//);
    const busRow = bus.closest('li') as HTMLElement;
    expect(busRow).toHaveTextContent('Timetable only');
    expect(busRow).toHaveTextContent('Starts here');
    expect(within(busRow).getByRole('button', { name: 'Track this bus' })).toBeInTheDocument();
    const train = screen.getByRole('link', { name: rowName('10:05', 'WAT') });
    expect(train.getAttribute('href')).toMatch(/^\/train\/C10002\//);
    const trainRow = train.closest('li') as HTMLElement;
    expect(trainRow).toHaveTextContent('Scheduled');
    expect(within(trainRow).getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
  });
});

describe('TrainSearchForm: live fields', () => {
  it('shows live status and a next-day marker when the backend sends them', async () => {
    const body = JSON.stringify({
      results: [
        {
          uid: 'C1',
          scheduled: '00:20',
          stationCrs: 'MAN',
          originCrs: 'EUS',
          originName: 'London Euston',
          destinationCrs: 'WAT',
          destinationName: 'London Waterloo',
          destinationArrival: null,
          destinationArrivalDayOffset: 0,
          dayOffset: 1,
          live: {
            status: 'en_route',
            delayMinutes: -2,
            delayProvisional: false,
            cancelled: false,
            lastReportedLocation: null,
          },
        },
      ],
      nextCursor: null,
    });
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response(body, { status: 200 }) }));
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);
    await clickSearch();

    // The "+1" is visible but hidden from the link's name, which says it in words.
    const link = await screen.findByRole('link', { name: /^00:20 \(next day\) to London Waterloo/ });
    const row = link.closest('li') as HTMLElement;
    expect(within(row).getByText('+1')).toBeInTheDocument();
    expect(row).toHaveTextContent('2 min early');
    expect(row).toHaveTextContent('From London Euston');
  });
});

describe('TrainSearchForm advanced options', () => {
  afterEach(() => {
    window.history.replaceState(null, '', '/');
  });

  /** Opens the Filters panel (once live) and then Advanced options. */
  async function openAdvanced() {
    openFiltersIfCompact();
    fireEvent.click(screen.getByRole('button', { name: /^Advanced options/ }));
    await screen.findByRole('combobox', { name: 'Call at (in order)' });
  }

  function addTyped(label: string, code: string) {
    const input = screen.getByRole('combobox', { name: label });
    fireEvent.change(input, { target: { value: code } });
    fireEvent.keyDown(input, { key: 'Enter', code: 'Enter' });
  }

  it("offers the planner's four station lists on the station view, collapsed, scoped to after the station", async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);
    await awaitMountSettled();
    openFiltersIfCompact();
    expect(screen.getByRole('button', { name: 'Advanced options' })).toHaveAttribute('aria-expanded', 'false');
    await openAdvanced();
    for (const label of ['Call at (in order)', 'Pass through (in order)', 'Avoid completely', "Don't stop at"]) {
      expect(screen.getByRole('combobox', { name: label })).toBeInTheDocument();
    }
    // The planner's change options mean nothing for one train.
    expect(screen.queryByRole('combobox', { name: "Don't change at" })).not.toBeInTheDocument();
    expect(screen.getByText("These look at the train's route after MAN.")).toBeInTheDocument();
  });

  it('sends a station added to Call at with the departures search, and keeps it in the URL', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    window.history.replaceState(null, '', '/trains?station=MAN');
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);
    await awaitMountSettled();
    await openAdvanced();
    addTyped('Call at (in order)', 'rdg');
    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&calls_at=RDG&across_midnight=true'),
    );
    await waitFor(() => expect(window.location.search).toBe('?station=MAN&calls_at=RDG'));
  });

  it('restores every list from the URL, sends them, and names them while collapsed', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <TrainSearchForm
        view="between"
        initialStation="MAN"
        initialStopsAt="WAT"
        initialStopFilters={{
          ...noFilters,
          callsAt: ['RDG', 'group:LON'],
          via: ['OXF'],
          avoid: ['BHM'],
          avoidStop: ['CRE'],
        }}
      />,
    );
    await awaitMountSettled();
    expect(searchCallUrl(fetchMock)).toBe(
      '/api/trains/search?station=MAN&stops_at=WAT&calls_at=RDG%2Cgroup%3ALON&via=OXF&avoid=BHM&avoid_stop=CRE&across_midnight=true',
    );
    openFiltersIfCompact();
    expect(screen.getByRole('button', { name: 'Advanced options (4 set)' })).toBeInTheDocument();
    expect(
      screen.getByText("Call at RDG, group:LON · Pass through OXF · Avoid BHM · Don't stop at CRE"),
    ).toBeInTheDocument();
  });

  it('scopes the lists to before the station on the arrivals board, and sends them there', async () => {
    const base = mockFetchByUrl();
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      if (url.startsWith('/api/trains/arrivals'))
        return Promise.resolve(new Response(JSON.stringify({ results: [], nextCursor: null }), { status: 200 }));
      return base(input);
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <TrainSearchForm
        initialStation="MAN"
        initialBoard="arrivals"
        initialStopFilters={{ ...noFilters, via: ['OXF'] }}
      />,
    );
    await waitFor(() =>
      expect(fetchMock.mock.calls.map((args) => String(args[0]))).toContain('/api/trains/arrivals?station=MAN&via=OXF'),
    );
    await openAdvanced();
    expect(screen.getByText("These look at the train's route before it reaches MAN.")).toBeInTheDocument();
    // One line on this board: every list looks before the station, so
    // "Previously calls at" (the same as "Call at" here) is left out.
    expect(screen.queryByText(/To find a particular train/)).not.toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'Previously calls at (in order)' })).not.toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Previously stops at (in order)' })).toBeInTheDocument();
  });

  it('offers Previously calls at and Previously stops at before the station, and sends them', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    window.history.replaceState(null, '', '/trains?station=MAN');
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);
    await awaitMountSettled();
    await openAdvanced();
    expect(
      screen.getByText('To find a particular train, these look at where it has been before MAN.'),
    ).toBeInTheDocument();
    expect(screen.getByText("These look at the train's route after MAN.")).toBeInTheDocument();
    addTyped('Previously calls at (in order)', 'eus');
    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&previously_calls_at=EUS&across_midnight=true',
      ),
    );
    addTyped('Previously stops at (in order)', 'crE');
    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&previously_calls_at=EUS&previously_stops_at=CRE&across_midnight=true',
      ),
    );
    await waitFor(() =>
      expect(window.location.search).toBe('?station=MAN&previously_calls_at=EUS&previously_stops_at=CRE'),
    );
  });

  it('sends the Previously lists between two stations, before the first', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <TrainSearchForm
        view="between"
        initialStation="MAN"
        initialStopsAt="WAT"
        initialStopFilters={{ ...noFilters, previouslyStopsAt: ['group:LON', 'CRE'] }}
      />,
    );
    await awaitMountSettled();
    expect(searchCallUrl(fetchMock)).toBe(
      '/api/trains/search?station=MAN&stops_at=WAT&previously_stops_at=group%3ALON%2CCRE&across_midnight=true',
    );
    await openAdvanced();
    expect(
      screen.getByText('To find a particular train, these look at where it has been before MAN.'),
    ).toBeInTheDocument();
  });

  it('shows a Previously calls at in force on the arrivals board rather than hiding it', async () => {
    const base = mockFetchByUrl();
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      if (url.startsWith('/api/trains/arrivals'))
        return Promise.resolve(new Response(JSON.stringify({ results: [], nextCursor: null }), { status: 200 }));
      return base(input);
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <TrainSearchForm
        initialStation="MAN"
        initialBoard="arrivals"
        initialStopFilters={{ ...noFilters, previouslyCallsAt: ['EUS'] }}
      />,
    );
    await waitFor(() =>
      expect(fetchMock.mock.calls.map((args) => String(args[0]))).toContain(
        '/api/trains/arrivals?station=MAN&previously_calls_at=EUS',
      ),
    );
    await openAdvanced();
    expect(screen.getByRole('combobox', { name: 'Previously calls at (in order)' })).toBeInTheDocument();
    expect(screen.getByText(/On the arrivals board this is the same as "Call at"/)).toBeInTheDocument();
  });

  it('refuses a Previously entry that is the station searched, and says why', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <TrainSearchForm initialStation="MAN" initialStopFilters={{ ...noFilters, previouslyStopsAt: ['MAN'] }} />,
    );
    openFiltersIfCompact();
    expect(screen.getByRole('button', { name: 'Advanced options (1 set, 1 to fix)' })).toBeInTheDocument();
    await openAdvanced();
    expect(
      screen.getByText('MAN is the station on this board: list a station elsewhere on the route.'),
    ).toBeInTheDocument();
    expect(searchCallUrls(fetchMock)).toHaveLength(0);
  });

  it('keeps Starts at in Advanced options, off the main form', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);
    expect(screen.queryByRole('combobox', { name: /Starts at/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /^Advanced options/ }));
    expect(await screen.findByRole('combobox', { name: 'Starts at' })).toBeInTheDocument();
  });

  it('sends Starts at as origin and Terminates at as terminates_at, and keeps them in the URL', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    window.history.replaceState(null, '', '/trains?station=MAN');
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);
    await awaitMountSettled();
    await openAdvanced();
    fireEvent.change(screen.getByRole('combobox', { name: 'Starts at' }), { target: { value: 'eus' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'Terminates at' }), { target: { value: 'wat' } });
    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&origin=EUS&terminates_at=WAT&across_midnight=true',
      ),
    );
    await waitFor(() => expect(window.location.search).toBe('?station=MAN&origin=EUS&terminates_at=WAT'));
  });

  it('refuses a half-typed Starts at, and a Terminates at the departures board avoids', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <TrainSearchForm
        initialStation="MAN"
        initialStopFilters={{ ...noFilters, startsAt: ['EU'], terminatesAt: ['WAT'], avoidStop: ['WAT'] }}
      />,
    );
    openFiltersIfCompact();
    expect(screen.getByRole('button', { name: 'Advanced options (3 set, 2 to fix)' })).toBeInTheDocument();
    await openAdvanced();
    expect(screen.getByText('Must be a 3-letter station code')).toBeInTheDocument();
    expect(
      screen.getByText('WAT is also in "Avoid completely" or "Don\'t stop at": a train can\'t both end there and not.'),
    ).toBeInTheDocument();
    expect(searchCallUrls(fetchMock)).toHaveLength(0);
  });

  it('keeps Starts at and Terminates at on the arrivals board', async () => {
    const base = mockFetchByUrl();
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      if (url.startsWith('/api/trains/arrivals'))
        return Promise.resolve(new Response(JSON.stringify({ results: [], nextCursor: null }), { status: 200 }));
      return base(input);
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <TrainSearchForm
        initialStation="MAN"
        initialBoard="arrivals"
        initialStopFilters={{ ...noFilters, startsAt: ['EUS'], terminatesAt: ['MAN'] }}
      />,
    );
    await waitFor(() =>
      expect(fetchMock.mock.calls.map((args) => String(args[0]))).toContain(
        '/api/trains/arrivals?station=MAN&origin=EUS&terminates_at=MAN',
      ),
    );
    await openAdvanced();
    expect(screen.getByRole('combobox', { name: 'Starts at' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Terminates at' })).toBeInTheDocument();
  });

  it('refuses to search on a list that cannot be met, and says why on the field', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <TrainSearchForm
        view="between"
        initialStation="MAN"
        initialStopsAt="WAT"
        initialStopFilters={{ ...noFilters, avoid: ['WAT'] }}
      />,
    );
    openFiltersIfCompact();
    expect(screen.getByRole('button', { name: 'Advanced options (1 set, 1 to fix)' })).toBeInTheDocument();
    await openAdvanced();
    expect(screen.getByText("WAT is where you get off, so it can't be avoided.")).toBeInTheDocument();
    expect(searchCallUrls(fetchMock)).toHaveLength(0);
    // Removing it searches.
    fireEvent.click(screen.getByRole('button', { name: 'Remove WAT from avoided stations' }));
    await waitFor(() => expect(searchCallUrls(fetchMock)).toHaveLength(1));
  });

  it('clears the lists with the other filters', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" initialStopFilters={{ ...noFilters, avoid: ['BHM'] }} />);
    await awaitMountSettled();
    expect(searchCallUrl(fetchMock)).toContain('avoid=BHM');
    openFiltersIfCompact();
    fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }));
    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&across_midnight=true'));
  });
});

const noFilters = {
  callsAt: [],
  via: [],
  avoid: [],
  avoidStop: [],
  previouslyCallsAt: [],
  previouslyStopsAt: [],
  startsAt: [],
  terminatesAt: [],
};
