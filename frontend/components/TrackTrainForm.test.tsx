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

describe('TrackTrainForm', () => {
  beforeEach(() => {
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
  });

  it('pre-fills the origin field from initialOrigin', async () => {
    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    expect(screen.getByRole('combobox', { name: /Origin CRS code/ })).toHaveValue('WAT');
    // A valid `initialOrigin` fires the departures effect on mount --
    // `waitFor` lets that resolve within `act(...)` before the test ends,
    // avoiding a spurious "not wrapped in act" warning.
    await waitFor(() => expect(fetch).toHaveBeenCalled());
  });

  it('disables submit until the origin is a valid 3-letter code and a departure is picked', () => {
    renderWithMantine(<TrackTrainForm />);
    expect(screen.getByRole('button', { name: /Track this train/ })).toBeDisabled();
  });

  it('shows a field error for a non-3-letter origin code', () => {
    renderWithMantine(<TrackTrainForm />);
    const field = screen.getByRole('combobox', { name: /Origin CRS code/ });
    fireEvent.change(field, { target: { value: 'WATERLOO' } });
    fireEvent.blur(field);
    expect(screen.getByText('Must be a 3-letter CRS code')).toBeInTheDocument();
  });

  it('does not show the origin error while still typing (no blur fired)', async () => {
    renderWithMantine(<TrackTrainForm />);
    fireEvent.change(screen.getByRole('combobox', { name: /Origin CRS code/ }), { target: { value: 'Wok' } });
    expect(screen.queryByText('Must be a 3-letter CRS code')).not.toBeInTheDocument();
    // 'Wok' is a valid CRS -- see the previous test's comment on why this
    // awaits the departures effect before the test ends.
    await waitFor(() => expect(fetch).toHaveBeenCalled());
  });

  it('shows no error on blur when the origin is a valid 3-letter code', async () => {
    renderWithMantine(<TrackTrainForm />);
    const field = screen.getByRole('combobox', { name: /Origin CRS code/ });
    fireEvent.change(field, { target: { value: 'WAT' } });
    fireEvent.blur(field);
    expect(screen.queryByText('Must be a 3-letter CRS code')).not.toBeInTheDocument();
    // See the earlier "does not show the origin error" test's comment on
    // why this awaits the departures effect before the test ends.
    await waitFor(() => expect(fetch).toHaveBeenCalled());
  });

  it('selecting an origin suggestion (via onChange) still submits the resolved origin_crs', async () => {
    const fetchMock = vi.mocked(fetch);
    // `mockImplementation`, not `mockResolvedValue`: this picker's own
    // departures effect and the eventual `/api/Train/track` POST both read
    // a `Response` body in this test, and a `Response` body can only be
    // consumed once -- `mockResolvedValue` would hand out the *same*
    // instance to both calls. A factory gives each `fetch` call its own
    // fresh, unconsumed `Response`.
    fetchMock.mockImplementation(() =>
      Promise.resolve(new Response(JSON.stringify({ trackingId: 42, resolutionStatus: 'pending' }), { status: 200 })),
    );

    renderWithMantine(<TrackTrainForm />);
    fireEvent.change(screen.getByRole('combobox', { name: /Origin CRS code/ }), { target: { value: 'WOK' } });
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
    const fetchMock = vi.mocked(fetch);
    // See the previous test's comment: a fresh `Response` per call, since
    // the departures effect and the submit POST both read a body.
    fetchMock.mockImplementation(() =>
      Promise.resolve(new Response(JSON.stringify({ trackingId: 42, resolutionStatus: 'pending' }), { status: 200 })),
    );

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
    const fetchMock = vi.mocked(fetch);
    // See the earlier "selecting an origin suggestion" test's comment: a
    // fresh `Response` per call, since the departures effect and the
    // submit POST both read a body.
    fetchMock.mockImplementation(() =>
      Promise.resolve(new Response(JSON.stringify({ trackingId: 42, resolutionStatus: 'pending' }), { status: 200 })),
    );

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
    fireEvent.change(screen.getByRole('combobox', { name: /Destination CRS code/ }), { target: { value: 'WOK' } });
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
    expect(screen.getByRole('combobox', { name: /Origin CRS code/ })).toHaveValue('WAT');
    expect(screen.getByRole('combobox', { name: /Destination CRS code/ })).toHaveValue('WOK');
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
    const fetchMock = vi.mocked(fetch);
    // See the earlier "selecting an origin suggestion" test's comment: a
    // fresh `Response` per call, since the departures effect and the
    // submit POST both read a body.
    fetchMock.mockImplementation(() =>
      Promise.resolve(new Response(JSON.stringify({ trackingId: 42, resolutionStatus: 'pending' }), { status: 200 })),
    );

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
    const fetchMock = vi.mocked(fetch);
    // See the earlier "selecting an origin suggestion" test's comment: a
    // fresh `Response` per call, since the departures effect and the
    // submit POST both read a body.
    fetchMock.mockImplementation(() =>
      Promise.resolve(new Response(JSON.stringify({ trackingId: 7, resolutionStatus: 'pending' }), { status: 200 })),
    );

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
    // See the earlier "selecting an origin suggestion" test's comment: a
    // fresh `Response` per call, since the departures effect and the
    // submit POST both read a body.
    fetchMock.mockImplementation(() =>
      Promise.resolve(new Response(JSON.stringify({ trackingId: 99, resolutionStatus: 'pending' }), { status: 200 })),
    );

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
      fireEvent.change(screen.getByRole('combobox', { name: /Origin CRS code/ }), { target: { value: 'WAT' } });

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
      expect(screen.getByRole('combobox', { name: /Destination CRS code/ })).toHaveValue('');
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

      expect(screen.getByRole('combobox', { name: /Destination CRS code/ })).toHaveValue('BSK');
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
      expect(screen.getByRole('combobox', { name: /Destination CRS code/ })).toHaveValue('BSK');

      fireEvent.change(screen.getByRole('combobox', { name: /Origin CRS code/ }), { target: { value: 'EDB' } });
      // 'EDB' re-fires the departures effect for the new origin -- await
      // its resolution before asserting, so the assertions below observe
      // settled state and the pending state update doesn't leak past this
      // test's end (see the "pre-fills the origin field" test's comment).
      await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/stations/EDB/departures', expect.anything()));

      expect(screen.getByRole('combobox', { name: /Destination CRS code/ })).toHaveValue('BSK');
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

      expect(screen.getByRole('combobox', { name: /Destination CRS code/ })).toHaveValue('CRE');
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
      const destinationField = screen.getByRole('combobox', { name: /Destination CRS code/ });
      fireEvent.change(destinationField, { target: { value: 'EXISTING' } });

      const row = await screen.findByRole('button', { name: /09:00/ });
      fireEvent.click(row);

      expect(destinationField).toHaveValue('EXISTING');
      const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
      expect(picker.value).toMatch(/09:00:00$/);
    });
  });
});
