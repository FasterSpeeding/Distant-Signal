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

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
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
    // Every row's `TrackThisTrainButton` now prefetches the caller's groups
    // (`useGroupSummaries`) on mount -- see TrackThisTrainButton.test.tsx's
    // own `mockFetchByUrl` for the same addition. Empty-array 200: this
    // file's tests are only about search/pagination/track-action wiring,
    // never about the shared-groups prompt.
    if (url === '/api/groups') return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
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
    renderWithMantine(<TrainSearchForm initialStation="man" initialOrigin="eus" initialStopsAt="OXF" />);

    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('Latest departure (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&origin=EUS&stops_at=OXF&from=09%3A00&to=12%3A00',
      ),
    );
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

    it('renders every time filter as a native time input with its own picker button', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      // `initialStopsAt` so the arrival pair (which only renders once Stops
      // at is filled in) is on screen alongside the departure pair -- all
      // four at once is the case where their button names have to differ.
      renderWithMantine(<TrainSearchForm initialStation="MAN" initialStopsAt="WAT" />);

      for (const { label, name } of TIME_FIELDS) {
        const input = screen.getByLabelText(label);
        expect(input).toHaveAttribute('type', 'time');
        // `step=60` is what keeps a native time input's value at "HH:MM" --
        // the exact shape `from`/`to`/`arrival_from`/`arrival_to` are parsed
        // as server-side. A seconds segment would put "HH:MM:SS" on the wire.
        expect(input).toHaveAttribute('step', '60');
        // `getByRole` throws on more than one match, so this doubles as the
        // assertion that the four buttons are uniquely named.
        expect(screen.getByRole('button', { name: `Pick ${name}` })).toBeInTheDocument();
      }
    });

    it('keeps each time filter optional and individually clearable', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);

      fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), {
        target: { value: '09:00' },
      });
      fireEvent.change(screen.getByLabelText('Latest departure (optional)'), {
        target: { value: '12:00' },
      });
      // Driven through the field's real clear button rather than a
      // synthetic empty-string change event, because that button is the
      // only way back to empty on a touch device (a native time input
      // opens a wheel picker with no keyboard to Backspace with, and
      // Mantine's own stylesheet hides the native clear control).
      fireEvent.click(screen.getByRole('button', { name: 'Clear earliest departure' }));

      // Only the cleared field's param is dropped; the other survives.
      fireEvent.click(screen.getByRole('button', { name: 'Search' }));

      await waitFor(() =>
        expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&to=12%3A00'),
      );
    });

    it('offers a clear button only on the time filters that have a value', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);

      expect(screen.queryByRole('button', { name: 'Clear earliest departure' })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Clear latest departure' })).not.toBeInTheDocument();

      fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), {
        target: { value: '09:00' },
      });

      expect(screen.getByRole('button', { name: 'Clear earliest departure' })).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Clear latest departure' })).not.toBeInTheDocument();
    });

    it('still flags a time the wire format does not accept', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<TrainSearchForm initialStation="MAN" />);

      // `"09:00:30"` is a VALID HTML time string, so a `type="time"`
      // input's own value sanitization lets it straight through to
      // `onChange` (jsdom does too -- verified, unlike outright garbage
      // such as `"25:99"`, which sanitizes to `""` below). It is still not
      // a value this API accepts: `from`/`to`/`arrival_from`/`arrival_to`
      // are parsed as bare `"HH:MM"`. So the component's own
      // `TIME_PATTERN` check is not dead weight after the swap to a picker
      // -- this is the case it still catches.
      fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), {
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
      fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), {
        target: { value: '25:99' },
      });
      expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('');

      fireEvent.click(screen.getByRole('button', { name: 'Search' }));

      await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN'));
    });

    it('blocks the search on an unacceptable ARRIVAL time too, not just a departure one', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<TrainSearchForm initialStation="MAN" initialStopsAt="WAT" />);

      // The arrival pair is conditionally rendered, so its contribution to
      // `canSearch` is easy to lose without noticing -- pinned separately
      // from the departure pair's for that reason.
      fireEvent.change(screen.getByLabelText('Latest arrival (optional)'), {
        target: { value: '09:30:15' },
      });

      expect(screen.getByText('Must be a time like 09:00')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    });
  });

  it('renders one row per result, with time, origin and destination', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('08:22 · EUS → MAN → WAT')).toBeInTheDocument();
    expect(screen.getByText('10:05 · CRE → MAN → WAT')).toBeInTheDocument();
  });

  // Regression guard for the results list being hard-clipped at a fixed
  // height. The rows used to sit inside a `<ScrollArea mah={420}
  // offsetScrollbars>`, whose root is `overflow: hidden` while its viewport
  // is `height: 100%`; against a root whose own `height` stays `auto` that
  // percentage resolves to `auto`, so the viewport never overflowed itself
  // (nothing scrolled) and the root simply clipped everything past 420px --
  // with no scrollbar to hint at it, since Mantine sizes its own from
  // `scrollHeight` vs `clientHeight`, equal in that state. jsdom does no
  // layout, so this asserts the *structure* that caused it instead.
  //
  // Note this also rejects `ScrollArea.Autosize` -- the component that
  // *would* cap the height correctly. Deliberate, matching
  // `IncidentSearchForm.test.tsx`'s identical guard: the choice here is "no
  // nested scroller at all, the page scrolls", and `TrainSearchForm.tsx`'s
  // own comment records why.
  it('renders the results list in the page flow, with no fixed-height or scroll-container ancestor', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('08:22 · EUS → MAN → WAT');

    const list = document.querySelector('[data-train-results]');
    expect(list).not.toBeNull();
    const form = (list as HTMLElement).closest('form');
    expect(form).not.toBeNull();

    // Walk the list itself plus every ancestor up to (and including) the
    // form -- a clip anywhere on that chain hides rows just as effectively
    // as one on the list. (Above the form is this component's caller, which
    // a unit test can't see.)
    for (
      let node: HTMLElement | null = list as HTMLElement;
      node !== null;
      node = node === form ? null : (node.parentElement as HTMLElement | null)
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

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('08:22 · EUS → MAN → WAT');
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await screen.findByText('11:40 · EUS → MAN → WAT');

    // Page 2's rows must land in the SAME in-flow list as page 1's, so the
    // page's own scrollbar reaches them -- under the removed `ScrollArea`
    // each "Load more" appended straight into the clipped region.
    const list = document.querySelector('[data-train-results]') as HTMLElement;
    for (const label of ['08:22 · EUS → MAN → WAT', '10:05 · CRE → MAN → WAT', '11:40 · EUS → MAN → WAT']) {
      expect(list.contains(screen.getByText(label))).toBe(true);
    }
  });

  // The row header can no longer be `wrap="nowrap"`: without the removed
  // `ScrollArea` viewport to absorb it into its own horizontal scroll, a
  // summary plus "View live status" plus a "Track this train" button is
  // wider than a ~360px screen, and a `nowrap` flex item's `min-width:
  // auto` floor would push the whole page sideways. jsdom lays nothing out,
  // so this is a tripwire against silent reintroduction -- the reasoning
  // lives in `TrainSearchForm.tsx`'s own comment on this `Group`.
  it('lets a result row wrap rather than forcing its actions onto the summary line', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    const summary = await screen.findByText('08:22 · EUS → MAN → WAT');
    const row = summary.parentElement as HTMLElement;

    // Mantine's `Group` resolves its `wrap` prop into the inline
    // `--group-wrap` custom property (`Group.mjs`'s `varsResolver`), which
    // its stylesheet feeds to `flex-wrap` -- so that variable, not
    // `style.flexWrap`, is where `wrap="nowrap"` would show up.
    expect(row.style.getPropertyValue('--group-wrap')).toBe('wrap');
    // ...and the actions still read flush right on whichever line they land
    // on, which `justify="space-between"` would NOT do once wrapped.
    const actions = screen.getAllByRole('link', { name: 'View live status' })[0]
      .parentElement as HTMLElement;
    expect(actions.style.marginInlineStart).toBe('auto');
    // ...and a wrapped actions line stays visually tied to ITS summary
    // rather than to the next train's. `Group`'s default `md` gap applies
    // on the wrap axis too, which would put the actions 16px below their
    // own summary but only 10px (the enclosing `Stack`'s `xs`) above the
    // row beneath -- see `TrainSearchForm.tsx`'s own comment on this
    // `Group`, and `IncidentSearchForm.tsx`'s identical `rowGap` note.
    expect(row.style.rowGap).toBe('4px');
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
    // The button doesn't just vanish -- the list says it is complete.
    expect(
      screen.getByText("You've reached the end — no more scheduled trains match those filters."),
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

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    // While a next page exists, the end must not be claimed.
    expect(await screen.findByRole('button', { name: 'Load more' })).toBeInTheDocument();
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(
      await screen.findByText("You've reached the end — no more scheduled trains match those filters."),
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

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText("Couldn't load more results. Try again.")).toBeInTheDocument();
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Load more' })).toBeEnabled();
    // A failed next page must not wipe out the page already on screen, nor
    // escalate to the whole-search error state.
    expect(screen.getByText('08:22 · EUS → MAN → WAT')).toBeInTheDocument();
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

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    await screen.findByText("Couldn't load more results. Try again.");

    failNextPage = false;
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(screen.queryByText("Couldn't load more results. Try again.")).not.toBeInTheDocument(),
    );
  });

  it('shows neither Load more nor the end-of-results line when the search matched nothing', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({ search: () => new Response(searchBody([]), { status: 200 }) }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

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
      if (searchCalls === 1) {
        return Promise.resolve(new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }));
      }
      // Call 2 is page 2 of search 1, held open; call 3 is search 2.
      if (searchCalls === 2) return pageTwo;
      return Promise.resolve(new Response(searchBody(PAGE_THREE, null), { status: 200 }));
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' })); // page 2 of search 1
    fireEvent.click(screen.getByRole('button', { name: 'Search' })); // search 2
    await screen.findByText('13:15 · CRE → MAN → WAT');

    resolvePageTwo(new Response(searchBody(PAGE_TWO, 'CURSOR2'), { status: 200 }));

    await waitFor(() =>
      expect(
        screen.getByText("You've reached the end — no more scheduled trains match those filters."),
      ).toBeInTheDocument(),
    );
    expect(screen.queryByText('11:40 · EUS → MAN → WAT')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('sends stops_at uppercased when entered directly (not just via a suggestion pick)', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.change(screen.getByRole('combobox', { name: 'Stops at (optional)' }), {
      target: { value: 'rdg' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&stops_at=RDG'),
    );
  });

  it('does not render the arrival-time filter until a station is entered in Stops at', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    expect(screen.queryByLabelText('Earliest arrival (optional)')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Latest arrival (optional)')).not.toBeInTheDocument();
  });

  it('renders the arrival-time filter once a station is entered in Stops at', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" initialStopsAt="WAT" />);

    expect(screen.getByLabelText('Earliest arrival (optional)')).toBeInTheDocument();
    expect(screen.getByLabelText('Latest arrival (optional)')).toBeInTheDocument();
  });

  it('sends arrival_from/arrival_to only when stops_at is set', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="man" initialStopsAt="wat" />);

    fireEvent.change(screen.getByLabelText('Earliest arrival (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('Latest arrival (optional)'), { target: { value: '09:30' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&stops_at=WAT&arrival_from=09%3A00&arrival_to=09%3A30',
      ),
    );
  });

  it('drops any previously-entered arrival-time filter once Stops at is cleared', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" initialStopsAt="RDG" />);

    fireEvent.change(screen.getByLabelText('Earliest arrival (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'Stops at (optional)' }), {
      target: { value: '' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN'));
  });
});
