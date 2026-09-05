import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import dayjs from 'dayjs';
import { renderWithMantine } from '@/test/render';
import { TrackTrainForm } from './TrackTrainForm';

/** Routes a mocked `fetch` call by URL: `/api/stations/{crs}/departures`
 * (LDBWS), `/api/stations/{crs}/schedule-departures` (the CIF fallback,
 * this task), suggestion fetches, and everything else (the track-submit
 * call). `departures` defaults to an inert empty-array 200 so a test only
 * needs to override the branch it actually cares about; `scheduleDeparatures`
 * has no default -- a test that expects the CIF fallback to fire but
 * doesn't configure it will throw loudly rather than silently returning
 * something misleading, since most tests never expect a 404 from the
 * `departures` fetch at all. */
function mockFetchByUrl(
  options: {
    departures?: () => Response;
    scheduleDepartures?: () => Response;
  } = {},
) {
  const { departures = () => new Response(JSON.stringify([]), { status: 200 }), scheduleDepartures } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (/\/api\/stations\/[A-Za-z]{3}\/schedule-departures$/.test(url)) {
      if (!scheduleDepartures) {
        throw new Error(`unexpected schedule-departures fetch for ${url} -- this test did not configure one`);
      }
      return Promise.resolve(scheduleDepartures());
    }
    if (/\/api\/stations\/[A-Za-z]{3}\/departures$/.test(url)) {
      return Promise.resolve(departures());
    }
    if (url.startsWith('/api/stations?') || url.startsWith('/api/tocs?')) {
      return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    }
    return Promise.resolve(new Response(JSON.stringify({ trackingId: 42, resolutionStatus: 'pending' }), { status: 200 }));
  });
}

/** Picks out the `POST /api/Train/track` call's parsed body from a mocked
 * `fetch`. This picker's own departures effect (`useEffect` on
 * `[originCrs, originValid]`) now also fires a `fetch` for any valid
 * origin CRS -- including on initial mount, for `initialOrigin` -- so a
 * plain `fetchMock.mock.calls[0]` is no longer reliably the submit call;
 * every pre-existing test that reads the submitted body needs to find it
 * by URL instead of by position. */
function trackCallBody(fetchMock: ReturnType<typeof vi.fn>) {
  const call = fetchMock.mock.calls.find((args: unknown[]) => args[0] === '/api/Train/track');
  if (!call) throw new Error('no /api/Train/track call recorded');
  const [, init] = call as [string, RequestInit];
  return JSON.parse(init!.body as string);
}

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/track',
  useSearchParams: () => new URLSearchParams(''),
}));

// `DateTimePicker`'s labelled control is a `<button>` that opens a popover
// calendar dialog, not a text `<input>` -- `fireEvent.change` with a
// `target.value` (the pattern every other field in this file uses) is a
// no-op on it, so there's no way to drive it the same way without either
// clicking through the real calendar/time-of-day controls (fragile, and
// tests nothing about *this* component -- `HistoryRangePicker.test.tsx`,
// this repo's one other dates-component test, never exercises typing into
// a picker either) or standing in a thin test-only replacement here. This
// stand-in keeps the exact contract `TrackTrainForm` actually depends on
// (a labelled control whose `onChange` is called with a `string | null`)
// so `fireEvent.change` continues to drive the real `onChange` handler --
// and therefore the real submit logic below -- without exercising Mantine's
// own (separately tested) calendar widget.
vi.mock('@mantine/dates', () => ({
  DateTimePicker: ({
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
      <label htmlFor="test-scheduled-departure">{label}</label>
      <input
        id="test-scheduled-departure"
        value={value ?? ''}
        onChange={(event) => onChange(event.target.value || null)}
      />
      {description && <p>{description}</p>}
    </div>
  ),
}));

// A fixed "now" well before every fixture departure time used below
// (earliest is '08:22') -- `scheduledDeparture` now defaults to `dayjs()`
// at mount (this task's own "default to now" fix), and the picker now
// filters rows by it (`matchesScheduledDeparture`), so every test that
// asserts a fixture row is visible without itself setting
// `scheduledDeparture` needs the real wall-clock time pinned to something
// earlier than all of them -- otherwise these tests would pass or fail
// depending on what time of day the suite happens to run. `shouldAdvanceTime`
// (same option `AutoRefresh.test.tsx` already uses) lets real `setTimeout`-driven
// async machinery (`waitFor`/`findBy*`) keep working normally while `Date`
// itself stays pinned near this fixed point.
const FIXED_NOW = '2026-09-05T00:01:00.000Z';

describe('TrackTrainForm', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(new Date(FIXED_NOW));
    // A resolved, empty-array 200 by default (not a bare `vi.fn()`, which
    // returns `undefined`) -- this picker's own departures effect now
    // fires a real `fetch` call for any test with a syntactically valid
    // origin CRS (including via `initialOrigin`), even tests that have
    // nothing to do with this feature, so every test needs *some* usable
    // default response unless it stubs its own. `[]` renders as the inert
    // "no live departures" state and reads nothing tests here assert on.
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(new Response(JSON.stringify([]), { status: 200 }))),
    );
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it('pre-fills the origin field from initialOrigin', async () => {
    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    expect(screen.getByRole('combobox', { name: /Origin station/ })).toHaveValue('WAT');
    // A valid `initialOrigin` fires the departures effect on mount --
    // `waitFor` lets that resolve within `act(...)` before the test ends,
    // avoiding a spurious "not wrapped in act" warning.
    await waitFor(() => expect(fetch).toHaveBeenCalled());
  });

  it('disables submit until the origin is a valid 3-letter code and a departure is picked', () => {
    renderWithMantine(<TrackTrainForm />);
    expect(screen.getByRole('button', { name: /Track this train/ })).toBeDisabled();
  });

  it('defaults the scheduled-departure field to the current time on mount, not null', () => {
    // Per the repo owner's own stated expectation ("which should be
    // defaulting to now tbh") -- `FIXED_NOW` is pinned above, so this
    // compares against the exact same `dayjs()` read the component's own
    // lazy `useState` initializer makes, not a fuzzy "close to now" check.
    renderWithMantine(<TrackTrainForm />);
    const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
    expect(picker.value).toBe(dayjs().format('YYYY-MM-DD HH:mm:ss'));
  });

  it('shows a field error for a non-3-letter origin code', () => {
    renderWithMantine(<TrackTrainForm />);
    const field = screen.getByRole('combobox', { name: /Origin station/ });
    fireEvent.change(field, { target: { value: 'WATERLOO' } });
    fireEvent.blur(field);
    expect(screen.getByText('Must be a 3-letter CRS code')).toBeInTheDocument();
  });

  it('does not show the origin error while still typing (no blur fired)', async () => {
    renderWithMantine(<TrackTrainForm />);
    fireEvent.change(screen.getByRole('combobox', { name: /Origin station/ }), { target: { value: 'Wok' } });
    expect(screen.queryByText('Must be a 3-letter CRS code')).not.toBeInTheDocument();
    // 'Wok' is a valid CRS -- see the previous test's comment on why this
    // awaits the departures effect before the test ends.
    await waitFor(() => expect(fetch).toHaveBeenCalled());
  });

  it('shows no error on blur when the origin is a valid 3-letter code', async () => {
    renderWithMantine(<TrackTrainForm />);
    const field = screen.getByRole('combobox', { name: /Origin station/ });
    fireEvent.change(field, { target: { value: 'WAT' } });
    fireEvent.blur(field);
    expect(screen.queryByText('Must be a 3-letter CRS code')).not.toBeInTheDocument();
    // See the earlier "does not show the origin error" test's comment on
    // why this awaits the departures effect before the test ends.
    await waitFor(() => expect(fetch).toHaveBeenCalled());
  });

  it('selecting an origin suggestion (via onChange) still submits the resolved origin_crs', async () => {
    // `mockFetchByUrl`, not a blanket `mockImplementation`: this picker's
    // own departures effect (fired once Origin resolves to 'WOK' below)
    // and the eventual `/api/Train/track` POST both read a `Response`
    // body, and a `Response` body can only be consumed once -- a blanket
    // factory handing back the SAME track-response shape for every URL
    // would also hand the departures fetch a non-array `rows`, which the
    // picker's own filtering now dereferences (`.filter`), so it must be
    // routed by URL instead. Each call still gets its own fresh,
    // unconsumed `Response`.
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<TrackTrainForm />);
    fireEvent.change(screen.getByRole('combobox', { name: /Origin station/ }), { target: { value: 'WOK' } });
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/track', expect.objectContaining({ method: 'POST' }));
    });
    const body = trackCallBody(fetchMock);
    expect(body.origin_crs).toBe('WOK');
  });

  it('leaving Destination and Operator empty omits both keys from the submitted body', async () => {
    // See the previous test's comment: routed by URL, not a blanket mock,
    // since the departures effect and the submit POST both read a body
    // and the picker now dereferences `rows` as an array.
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/track', expect.objectContaining({ method: 'POST' }));
    });
    const body = trackCallBody(fetchMock);
    expect(body).not.toHaveProperty('destination_crs');
    expect(body).not.toHaveProperty('operator');
  });

  it('on success, POSTs to /api/Train/track and redirects to /train/by-id/{trackingId}', async () => {
    // See the earlier "selecting an origin suggestion" test's comment:
    // routed by URL, not a blanket mock.
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/track', expect.objectContaining({ method: 'POST' }));
    });
    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/train/by-id/42');
    });
  });

  it('on a 401, shows the login prompt modal and preserves the typed field values', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByRole('combobox', { name: /Destination station/ }), { target: { value: 'WOK' } });
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    expect(await screen.findByText('Log in to track this train.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Ftrack',
    );
    // Unlike PinToggle's toggle-and-forget click, the form's own input
    // must survive a 401 -- Decision 4's explicit "preserve typed values"
    // call.
    expect(screen.getByRole('combobox', { name: /Origin station/ })).toHaveValue('WAT');
    expect(screen.getByRole('combobox', { name: /Destination station/ })).toHaveValue('WOK');
  });

  it('on a 400, shows the server error message inline', async () => {
    // The exact copy is `crates/api/src/data/train_tracking.rs::validate_pin`'s
    // source of truth -- this is testing the pass-through (the backend's
    // 400 body is rendered verbatim), not owning the wording itself.
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response(
        'That departure time is more than 6 hours ago — trains can only be tracked within 6 hours of departure.',
        { status: 400 },
      ),
    );

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    expect(
      await screen.findByText(
        'That departure time is more than 6 hours ago — trains can only be tracked within 6 hours of departure.',
      ),
    ).toBeInTheDocument();
  });

  it('on a 500, shows a generic error message', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('internal error', { status: 500 }));

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    expect(await screen.findByText("Couldn't create the tracking pin. Try again.")).toBeInTheDocument();
  });

  it('on an empty-body 400, still shows the generic error message rather than nothing', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('', { status: 400 }));

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    expect(await screen.findByText("Couldn't create the tracking pin. Try again.")).toBeInTheDocument();
  });

  it('on a network failure, shows the generic error message instead of failing silently', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.reject(new Error('network down'))),
    );

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    expect(await screen.findByText("Couldn't create the tracking pin. Try again.")).toBeInTheDocument();
  });

  // Part A of the upload-first plan: `attachTicketId`, set when arriving
  // from a standalone ticket's own "find or track the train this ticket is
  // for" link.
  it('with attachTicketId: attaches the ticket to the new pin before redirecting', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input: RequestInfo | URL) => {
      const url = String(input);
      // The picker's own departures effect (fired for `initialOrigin="WAT"`
      // below) also goes through this mock -- routed to an inert empty
      // array first, same reasoning as `mockFetchByUrl`'s own default,
      // since the picker now dereferences `rows` as an array.
      if (/\/api\/stations\/[A-Za-z]{3}\/departures$/.test(url)) {
        return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
      }
      if (url === '/api/Train/track') {
        return Promise.resolve(new Response(JSON.stringify({ trackingId: 42, resolutionStatus: 'pending' }), { status: 200 }));
      }
      return Promise.resolve(new Response(JSON.stringify({ ticketId: 99, trackedTrainId: 42 }), { status: 200 }));
    });

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" attachTicketId={99} />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/tickets/99/attach',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trackingId: 42 }) }),
      );
    });
    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/train/by-id/42');
    });
  });

  it('with attachTicketId: a failed attach still redirects (tracking the train already succeeded)', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input: RequestInfo | URL) => {
      const url = String(input);
      // See the previous test's comment on why the departures fetch is
      // routed separately, to an inert empty array.
      if (/\/api\/stations\/[A-Za-z]{3}\/departures$/.test(url)) {
        return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
      }
      if (url === '/api/Train/track') {
        return Promise.resolve(new Response(JSON.stringify({ trackingId: 42, resolutionStatus: 'pending' }), { status: 200 }));
      }
      return Promise.resolve(new Response('ticket is already attached to a tracked train', { status: 409 }));
    });

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" attachTicketId={99} />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/train/by-id/42');
    });
  });

  it('without attachTicketId: never calls the attach route', async () => {
    // See the earlier "selecting an origin suggestion" test's comment:
    // routed by URL, not a blanket mock.
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/train/by-id/42');
    });
    expect(fetchMock).not.toHaveBeenCalledWith(expect.stringContaining('/attach'), expect.anything());
  });

  it('derives service_date from the picker\'s local wall-clock date, not the UTC date', async () => {
    // A local time just after midnight, near a UTC day boundary (e.g.
    // during BST, UTC+1): the naive `new Date(...).toISOString().slice(0,
    // 10)` approach would roll this back to '2026-08-28', the WRONG
    // calendar date the user actually picked.
    // See the earlier "selecting an origin suggestion" test's comment:
    // routed by URL, not a blanket mock.
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-29 00:30:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalled();
    });
    const body = trackCallBody(fetchMock);
    expect(body.service_date).toBe('2026-08-29');
  });

  it('the Now button fills the picker with a well-formed local wall-clock value and enables submit', async () => {
    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

    fireEvent.click(screen.getByRole('button', { name: 'Now' }));

    const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
    // 'YYYY-MM-DD HH:mm:ss', not an ISO string -- matching the exact shape
    // the real DateTimePicker produces (see this component's own
    // handleSubmit comment on why that distinction matters).
    expect(picker.value).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/);
    expect(screen.getByRole('button', { name: /Track this train/ })).not.toBeDisabled();
    // `initialOrigin="WAT"` fires the departures effect on mount -- see
    // the "pre-fills the origin field" test's comment.
    await waitFor(() => expect(fetch).toHaveBeenCalled());
  });

  it('submits successfully after clicking Now, sending a well-formed ISO scheduled_departure', async () => {
    const fetchMock = vi.mocked(fetch);
    // See the earlier "selecting an origin suggestion" test's comment on
    // why the departures fetch must be routed separately from the submit
    // POST -- this test additionally needs a specific `trackingId` (99)
    // in the submit response, so it routes explicitly rather than reusing
    // `mockFetchByUrl`'s fixed 42.
    fetchMock.mockImplementation((input: RequestInfo | URL) => {
      const url = String(input);
      if (/\/api\/stations\/[A-Za-z]{3}\/departures$/.test(url)) {
        return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
      }
      return Promise.resolve(new Response(JSON.stringify({ trackingId: 99, resolutionStatus: 'pending' }), { status: 200 }));
    });

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Now' }));
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/track', expect.objectContaining({ method: 'POST' }));
    });
    const body = trackCallBody(fetchMock);
    expect(body.service_date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(() => new Date(body.scheduled_departure).toISOString()).not.toThrow();
    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/train/by-id/99');
    });
  });

  describe('live departures picker', () => {
    const departures = [
      {
        serviceId: 'svc-cancelled',
        operator: 'ZA',
        destinationCrs: 'WAT',
        scheduled: '10:15',
        estimated: 'Cancelled',
        isCancelled: true,
        delayMinutes: 0,
        cancelReason: 'fleet issue',
        delayReason: null,
        skippedStations: [],
      },
      {
        serviceId: 'svc-on-time',
        operator: 'SW',
        destinationCrs: 'BSK',
        scheduled: '10:40',
        estimated: 'On time',
        isCancelled: false,
        delayMinutes: 0,
        cancelReason: null,
        delayReason: null,
        skippedStations: [],
      },
    ];

    it('typing a valid origin CRS triggers a departures fetch to /api/stations/{ORIGIN}/departures', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify([]), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm />);
      fireEvent.change(screen.getByRole('combobox', { name: /Origin station/ }), { target: { value: 'WAT' } });

      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith('/api/stations/WAT/departures', expect.anything());
      });
    });

    it('a 404 from both LDBWS and CIF renders the "no departure information" unavailable text', async () => {
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response('not found', { status: 404 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      expect(
        await screen.findByText('No departure information is available for this station — enter the details below.'),
      ).toBeInTheDocument();
    });

    it('a 200 [] response renders the "no live departures right now" text', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify([]), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      expect(
        await screen.findByText('No live departures currently on the board for this station right now.'),
      ).toBeInTheDocument();
    });

    it('renders a cancelled and an on-time departure, with the cancelled row not clickable', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      expect(await screen.findByText('Cancelled')).toBeInTheDocument();
      expect(screen.getByText('On time')).toBeInTheDocument();

      // The cancelled row has no button role (not clickable); the on-time
      // row does.
      expect(screen.queryAllByRole('button', { name: /10:15/ })).toHaveLength(0);
      expect(screen.getByRole('button', { name: /10:40/ })).toBeInTheDocument();

      // Clicking the cancelled row's text does not fill any field.
      fireEvent.click(screen.getByText(/10:15/));
      expect(screen.getByRole('combobox', { name: /Destination station/ })).toHaveValue('');
    });

    it('clicking a non-cancelled row fills destinationCrs/operator/scheduledDeparture', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      const onTimeRow = await screen.findByRole('button', { name: /10:40/ });
      // Reuses this file's existing "Now"-button test's technique (see
      // that test above) for reading "today" deterministically: rather
      // than mocking `dayjs()`/system time, compute the expected date via
      // the same `dayjs()` call the component itself makes, at the moment
      // of the assertion.
      const today = dayjs().format('YYYY-MM-DD');
      fireEvent.click(onTimeRow);

      expect(screen.getByRole('combobox', { name: /Destination station/ })).toHaveValue('BSK');
      expect(screen.getByRole('combobox', { name: /Operator/ })).toHaveValue('SW');
      const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
      expect(picker.value).toBe(`${today} 10:40:00`);
    });

    it('changing the origin away from a previously-picked value does not clear already-filled fields', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      const onTimeRow = await screen.findByRole('button', { name: /10:40/ });
      fireEvent.click(onTimeRow);
      expect(screen.getByRole('combobox', { name: /Destination station/ })).toHaveValue('BSK');

      fireEvent.change(screen.getByRole('combobox', { name: /Origin station/ }), { target: { value: 'EDB' } });
      // 'EDB' re-fires the departures effect for the new origin -- await
      // its resolution before asserting, so the assertions below observe
      // settled state and the pending state update doesn't leak past this
      // test's end (see the "pre-fills the origin field" test's comment).
      await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/stations/EDB/departures', expect.anything()));

      expect(screen.getByRole('combobox', { name: /Destination station/ })).toHaveValue('BSK');
      expect(screen.getByRole('combobox', { name: /Operator/ })).toHaveValue('SW');
      const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
      expect(picker.value).toMatch(/^\d{4}-\d{2}-\d{2} 10:40:00$/);
    });

    const scheduleDepartures: { uid: string; scheduled: string; destinationCrs: string | null }[] = [
      { uid: 'C11052', scheduled: '08:22', destinationCrs: 'CRE' },
      { uid: 'C99999', scheduled: '09:00', destinationCrs: null },
    ];

    it('a 404 from LDBWS followed by a CIF 200 renders the CIF picker with its staleness disclaimer, no badges', async () => {
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      expect(
        await screen.findByText(
          /Live departure boards aren't available for this station\. Showing the scheduled timetable/,
        ),
      ).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /08:22/ })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /09:00/ })).toBeInTheDocument();
      expect(screen.queryByText('On time')).not.toBeInTheDocument();
      expect(screen.queryByText('Cancelled')).not.toBeInTheDocument();
    });

    it('a 404 from LDBWS followed by a CIF 200 [] renders the shared "no live departures right now" text', async () => {
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response(JSON.stringify([]), { status: 200 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      expect(
        await screen.findByText('No live departures currently on the board for this station right now.'),
      ).toBeInTheDocument();
    });

    it('a non-404, non-ok LDBWS response does not fall back to CIF at all', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response('server error', { status: 500 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      expect(screen.queryByText('No departure information is available for this station — enter the details below.')).not.toBeInTheDocument();
      expect(screen.queryByText(/Showing the scheduled timetable/)).not.toBeInTheDocument();
      expect(screen.queryByText('No live departures currently on the board for this station right now.')).not.toBeInTheDocument();
    });

    it('clicking a CIF row with a real destinationCrs fills destination and scheduled departure, leaving operator untouched', async () => {
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      const row = await screen.findByRole('button', { name: /08:22/ });
      const today = dayjs().format('YYYY-MM-DD');
      fireEvent.click(row);

      expect(screen.getByRole('combobox', { name: /Destination station/ })).toHaveValue('CRE');
      expect(screen.getByRole('combobox', { name: /Operator/ })).toHaveValue('');
      const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
      expect(picker.value).toBe(`${today} 08:22:00`);
    });

    it('clicking a CIF row with a null destinationCrs leaves any existing destination untouched', async () => {
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      const destinationField = screen.getByRole('combobox', { name: /Destination station/ });
      fireEvent.change(destinationField, { target: { value: 'EXISTING' } });

      const row = await screen.findByRole('button', { name: /09:00/ });
      fireEvent.click(row);

      expect(destinationField).toHaveValue('EXISTING');
      const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
      expect(picker.value).toMatch(/09:00:00$/);
    });

    it('shows the picker container with a prompt before Origin is filled in', () => {
      renderWithMantine(<TrackTrainForm />);
      expect(screen.getByText('Enter an origin station above to see upcoming departures.')).toBeInTheDocument();
    });

    it('filters LDBWS rows by a resolved Destination code, but not by still-partial text', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      await screen.findByRole('button', { name: /10:40/ });

      // Partial, unresolved text -- both rows still shown, per Decision 1's
      // "no filtering while the field still holds partial/typed-name text".
      fireEvent.change(screen.getByRole('combobox', { name: /Destination station/ }), { target: { value: 'Bo' } });
      expect(screen.getByText(/10:15/)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /10:40/ })).toBeInTheDocument();

      // A resolved 3-letter code narrows to the row whose destinationCrs
      // matches it, case-insensitively.
      fireEvent.change(screen.getByRole('combobox', { name: /Destination station/ }), { target: { value: 'bsk' } });
      expect(screen.queryByText(/10:15/)).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: /10:40/ })).toBeInTheDocument();
    });

    it('filters LDBWS rows by a resolved Operator code', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      await screen.findByRole('button', { name: /10:40/ });

      fireEvent.change(screen.getByRole('combobox', { name: /Operator/ }), { target: { value: 'SW' } });

      // svc-cancelled's operator is 'ZA' -- filtered out; svc-on-time's is
      // 'SW' -- still shown.
      expect(screen.queryByText(/10:15/)).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: /10:40/ })).toBeInTheDocument();
    });

    it('selecting a real operator suggestion from the dropdown filters the picker by its bare code', async () => {
      // This is the exact interaction the bug report describes ("the
      // selected operator" isn't accounted for) -- typing a partial name
      // and clicking the rendered suggestion, not typing the bare code
      // directly (already covered by the previous test). `Autocomplete`'s
      // `data` for Operator is built as `{ value: s.code, label: s.code }`
      // (`TrackTrainForm.tsx`'s Operator field) -- `label` deliberately
      // equals the bare code, not the display name, so Mantine's own
      // `onOptionSubmit` (which inserts `optionsLockup[val].label`) fills
      // the field with `'SW'`, not `'South Western Railway'`, and the
      // existing `OPERATOR_PATTERN` match already applies -- no separate
      // fix was needed for this path, but it's the one the report actually
      // describes, so it gets its own direct coverage rather than relying
      // on the bare-code-typed test above to stand in for it.
      //
      // Real timers only for this one test: the debounced suggestion
      // fetch inside `useSuggestions` needs its `setTimeout` -> `fetch` ->
      // `.then` chain to actually flush, which fake timers (even with
      // `shouldAdvanceTime`) don't reliably drive end to end. Since real
      // timers mean `scheduledDeparture`'s "now" default is the real
      // wall-clock time (not the pinned `FIXED_NOW`), the departure time
      // is explicitly overridden below to today's midnight, right after
      // mount and before anything awaits, so the fixture rows stay visible
      // regardless of what time of day this test happens to run.
      vi.useRealTimers();
      const fetchMock = vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        if (/\/api\/stations\/[A-Za-z]{3}\/departures$/.test(url)) {
          return Promise.resolve(new Response(JSON.stringify(departures), { status: 200 }));
        }
        if (url.startsWith('/api/tocs?')) {
          return Promise.resolve(
            new Response(JSON.stringify([{ code: 'SW', name: 'South Western Railway' }]), { status: 200 }),
          );
        }
        return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
        target: { value: `${dayjs().format('YYYY-MM-DD')} 00:00:00` },
      });
      await screen.findByRole('button', { name: /10:40/ });

      const operatorField = screen.getByRole('combobox', { name: /Operator/ });
      fireEvent.change(operatorField, { target: { value: 'south' } });

      // `hidden: true` -- same jsdom-only workaround `CustomLineForm.test.tsx`/
      // `StationSearchForm.test.tsx` already use for this exact Autocomplete
      // dropdown: jsdom's stubbed `ResizeObserver` (`vitest.setup.ts`) never
      // fires, so Floating UI never flips the dropdown's `display: none`
      // even once its data is non-empty -- a jsdom rendering limitation, not
      // a real browser behavior (`aria-expanded` is already `true` by this
      // point) or a bug in this component.
      const option = await screen.findByRole('option', { name: /South Western Railway/, hidden: true });
      fireEvent.click(option);

      expect(operatorField).toHaveValue('SW');
      // svc-cancelled's operator is 'ZA' -- filtered out by the now-resolved
      // Operator selection; svc-on-time's is 'SW' -- still shown.
      expect(screen.queryByText(/10:15/)).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: /10:40/ })).toBeInTheDocument();
    });

    it('changing the scheduled-departure time filters out earlier departures from the picker', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      await screen.findByRole('button', { name: /10:40/ });
      // `FIXED_NOW` (00:01) is well before either fixture departure, so
      // both are visible before narrowing the departure time at all.
      expect(screen.getByText(/10:15/)).toBeInTheDocument();

      // Narrow to a time between the two rows' scheduled times -- the
      // 10:15 departure has already left by 10:20, the 10:40 one hasn't.
      fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
        target: { value: '2026-09-05 10:20:00' },
      });

      expect(screen.queryByText(/10:15/)).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: /10:40/ })).toBeInTheDocument();
    });

    it('a Destination filter that matches no LDBWS row shows its own "no match" text, not the generic empty-board text', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      await screen.findByRole('button', { name: /10:40/ });

      fireEvent.change(screen.getByRole('combobox', { name: /Destination station/ }), { target: { value: 'ZZZ' } });

      expect(
        await screen.findByText("No upcoming departures match the destination and/or operator you've entered."),
      ).toBeInTheDocument();
      expect(screen.queryByText(/10:15/)).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /10:40/ })).not.toBeInTheDocument();
      expect(
        screen.queryByText('No live departures currently on the board for this station right now.'),
      ).not.toBeInTheDocument();
    });

    it('an Operator filter does not eliminate CIF rows -- CIF has no operator field to filter on', async () => {
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      await screen.findByRole('button', { name: /08:22/ });

      fireEvent.change(screen.getByRole('combobox', { name: /Operator/ }), { target: { value: 'SW' } });

      // Both CIF rows remain visible -- an Operator filter simply never
      // applies to a source that has no operator field at all.
      expect(screen.getByRole('button', { name: /08:22/ })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /09:00/ })).toBeInTheDocument();
    });

    it('a Destination filter can legitimately empty the CIF list, with its own "no match" text', async () => {
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      await screen.findByRole('button', { name: /08:22/ });

      // Matches neither 'CRE' nor the null-destination row.
      fireEvent.change(screen.getByRole('combobox', { name: /Destination station/ }), { target: { value: 'ZZZ' } });

      expect(
        await screen.findByText("No upcoming scheduled departures match the destination you've entered."),
      ).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /08:22/ })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /09:00/ })).not.toBeInTheDocument();
      // The staleness disclaimer is a property of the source, not of how
      // many rows survived filtering -- it still renders.
      expect(screen.getByText(/Live departure boards aren't available for this station/)).toBeInTheDocument();
    });
  });
});
