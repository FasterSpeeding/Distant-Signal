import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { ReactElement } from 'react';
import { screen, fireEvent, waitFor, act } from '@testing-library/react';
import dayjs from 'dayjs';
import { renderWithMantine } from '@/test/render';
import { GroupSummariesProvider } from '@/lib/useGroupSummaries';
import { TrackTrainForm } from './TrackTrainForm';
import type { GroupSummary } from '@/lib/types';

/** `useGroupSummaries` now reads from `GroupSummariesProvider`'s context
 * instead of fetching `/api/groups` itself (see `lib/useGroupSummaries.tsx`'s
 * own doc comment) -- most tests in this file never touch this and can keep
 * using plain `renderWithMantine` (no provider in the tree means
 * `useGroupSummaries` falls back to `[]`, the same zero-groups default
 * every pre-existing test here already exercised); only the "group-share
 * destination prompt" tests below need a real groups list, via this
 * helper. */
function renderWithGroups(ui: ReactElement, groups: GroupSummary[] | null) {
  return renderWithMantine(<GroupSummariesProvider groups={groups}>{ui}</GroupSummariesProvider>);
}

/** Routes a mocked `fetch` call by URL: `/api/stations/{crs}/departures`
 * (LDBWS), `/api/stations/{crs}/schedule-departures` (the CIF fallback,
 * this task), suggestion fetches, and everything else (the journey-submit
 * call, `POST /api/Journeys` since Task 15's `POST /Train/track` ->
 * `POST /Journeys` rewiring). `departures` defaults to an inert empty-array
 * 200 so a test only needs to override the branch it actually cares about;
 * `scheduleDeparatures` has no default -- a test that expects the CIF
 * fallback to fire but doesn't configure it will throw loudly rather than
 * silently returning something misleading, since most tests never expect a
 * 404 from the `departures` fetch at all. */
function mockFetchByUrl(
  options: {
    departures?: () => Response;
    scheduleDepartures?: () => Response;
  } = {},
) {
  const {
    departures = () => new Response(JSON.stringify([]), { status: 200 }),
    scheduleDepartures,
  } = options;
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
    return Promise.resolve(
      new Response(
        JSON.stringify({ journeyId: 99, legId: 1, trackingId: 42, resolutionStatus: 'pending' }),
        { status: 200 },
      ),
    );
  });
}

/** Picks out the `POST /api/Journeys` call's parsed body from a mocked
 * `fetch`. This picker's own departures effect (`useEffect` on
 * `[originCrs, originValid]`) now also fires a `fetch` for any valid
 * origin CRS -- including on initial mount, for `initialOrigin` -- so a
 * plain `fetchMock.mock.calls[0]` is no longer reliably the submit call;
 * every pre-existing test that reads the submitted body needs to find it
 * by URL instead of by position. */
function journeyCallBody(fetchMock: ReturnType<typeof vi.fn>) {
  const call = fetchMock.mock.calls.find((args: unknown[]) => args[0] === '/api/Journeys');
  if (!call) throw new Error('no /api/Journeys call recorded');
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
// `...(await importOriginal())`, not a bare replacement object (Task 15):
// this file now also renders `DatePickerInput` (the window-mode Date
// field) and, via `TimeFilterInput`, `TimeInput` -- both real `@mantine/
// dates` exports this component depends on. A bare `{ DateTimePicker:
// ... }` mock would replace the WHOLE module for every importer, leaving
// those two `undefined` and crashing window mode's render entirely, not
// just standing in for the one component this stand-in actually needs to
// replace. Same pattern `TrainSearchForm.test.tsx`'s own `DatePickerInput`
// mock already uses.
vi.mock('@mantine/dates', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@mantine/dates')>()),
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

  it('pre-fills the pin-mode destination field from initialDestination', () => {
    renderWithMantine(<TrackTrainForm initialOrigin="WAT" initialDestination="RDG" />);
    expect(screen.getByRole('combobox', { name: /Destination station \(optional\)/ })).toHaveValue('RDG');
  });

  it('pre-fills the window-mode destination and time-bound fields together', () => {
    renderWithMantine(
      <TrackTrainForm
        initialMode="window"
        initialOrigin="WAT"
        initialDestination="RDG"
        initialDepartAfter="08:00"
        initialArriveBefore="10:00"
      />,
    );
    expect(screen.getByRole('combobox', { name: /^Destination station$/ })).toHaveValue('RDG');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('08:00');
    expect(screen.getByLabelText('Latest arrival (optional)')).toHaveValue('10:00');
    // Bounds that weren't passed stay empty, not "undefined" leaking through.
    expect(screen.getByLabelText('Latest departure (optional)')).toHaveValue('');
    expect(screen.getByLabelText('Earliest arrival (optional)')).toHaveValue('');
  });

  it('leaves scheduled departure and window date at their own today/now defaults regardless of the new props', () => {
    renderWithMantine(
      <TrackTrainForm initialOrigin="WAT" initialDestination="RDG" initialDepartAfter="08:00" />,
    );
    // Judgment Call 4: no initialServiceDate/initialScheduledDeparture prop
    // exists at all -- "again" never carries the old date/time forward.
    const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
    expect(picker.value).not.toBe('');
  });

  // Task 3.6.14: this button used to stay `disabled` for as long as the
  // origin/departure weren't valid yet, which Mantine renders as
  // near-invisible light-grey-on-slightly-lighter-grey in dark mode. It's
  // no longer disabled for that reason at all (only while a submit is
  // actually in flight) -- an invalid press instead surfaces an inline
  // field error and does not call the API, so a click always gets a
  // visible result.
  it('is never disabled merely for an incomplete origin -- an invalid submit shows a field error instead', () => {
    renderWithMantine(<TrackTrainForm />);
    const button = screen.getByRole('button', { name: /Track this train/ });
    expect(button).not.toBeDisabled();
    fireEvent.click(button);
    expect(
      screen.getByText('Enter a valid origin station before tracking — pick one from the suggestions, or a 3-letter CRS code.'),
    ).toBeInTheDocument();
    expect(fetch).not.toHaveBeenCalledWith('/api/Journeys', expect.anything());
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

  it('shows an accessible "no matches" option instead of hiding the listbox when a search matches nothing', async () => {
    // Mantine's `Autocomplete` has no `nothingFoundMessage` prop at all
    // (unlike Select/MultiSelect) -- it hides its whole `role="listbox"`
    // dropdown outright whenever `data` is empty, leaving an open combobox
    // (`aria-expanded="true"`) with no `option`/`group` child, which fails
    // axe's `aria-required-children`. `withNoMatchPlaceholder`
    // (`lib/autocompleteNoMatch.ts`) works around the missing prop by
    // swapping in a single inert `role="option"` placeholder whenever the
    // real suggestions list is empty -- `mockFetchByUrl`'s own default
    // `/api/stations?`/`/api/tocs?` handler already returns `[]`.
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<TrackTrainForm />);
    const input = screen.getByRole('combobox', { name: /Origin station/ });
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'zzzzzz' } });
    // The placeholder is gated on a settled (non-loading) search (I2, the
    // 2026-09-17 whole-branch review) -- it must not appear until
    // `useSuggestions`'s 250ms debounce has actually elapsed.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    // Mantine's dropdown re-renders (its content briefly empties while
    // `active` is gated off during the loading window, then repopulates
    // with the placeholder) is present in the DOM but `display: none`
    // under jsdom (floating-ui's positioning never gets real layout info
    // here after that re-render) -- same as `StationSearchForm.test.tsx`'s
    // own analogous test, so the option is queried past Testing Library's
    // default visibility filter.
    expect(await screen.findByRole('option', { name: 'No matching stations', hidden: true })).toBeInTheDocument();
  });

  it('selecting an origin suggestion (via onChange) still submits the resolved originCrs', async () => {
    // `mockFetchByUrl`, not a blanket `mockImplementation`: this picker's
    // own departures effect (fired once Origin resolves to 'WOK' below)
    // and the eventual `/api/Journeys` POST both read a `Response`
    // body, and a `Response` body can only be consumed once -- a blanket
    // factory handing back the SAME journey-response shape for every URL
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
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys', expect.objectContaining({ method: 'POST' }));
    });
    const body = journeyCallBody(fetchMock);
    expect(body.leg.mode).toBe('pin');
    expect(body.leg.originCrs).toBe('WOK');
  });

  it('leaving Destination and Operator empty omits both keys from the submitted leg', async () => {
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
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys', expect.objectContaining({ method: 'POST' }));
    });
    const body = journeyCallBody(fetchMock);
    expect(body.leg).not.toHaveProperty('destinationCrs');
    expect(body.leg).not.toHaveProperty('operator');
    expect(body.leg).not.toHaveProperty('skippedStations');
  });

  it('on success, POSTs to /api/Journeys and redirects to /journeys/{journeyId}', async () => {
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
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys', expect.objectContaining({ method: 'POST' }));
    });
    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/journeys/99');
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
      if (url === '/api/Journeys') {
        return Promise.resolve(
          new Response(
            JSON.stringify({ journeyId: 99, legId: 1, trackingId: 42, resolutionStatus: 'pending' }),
            { status: 200 },
          ),
        );
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
      expect(pushMock).toHaveBeenCalledWith('/journeys/99');
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
      if (url === '/api/Journeys') {
        return Promise.resolve(
          new Response(
            JSON.stringify({ journeyId: 99, legId: 1, trackingId: 42, resolutionStatus: 'pending' }),
            { status: 200 },
          ),
        );
      }
      return Promise.resolve(new Response('ticket is already attached to a tracked train', { status: 409 }));
    });

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" attachTicketId={99} />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/journeys/99');
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
      expect(pushMock).toHaveBeenCalledWith('/journeys/99');
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
    const body = journeyCallBody(fetchMock);
    expect(body.leg.serviceDate).toBe('2026-08-29');
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

  it('submits successfully after clicking Now, sending a well-formed ISO scheduledDeparture', async () => {
    const fetchMock = vi.mocked(fetch);
    // See the earlier "selecting an origin suggestion" test's comment on
    // why the departures fetch must be routed separately from the submit
    // POST -- this test additionally needs a specific `journeyId`/
    // `trackingId` (199/99) in the submit response, so it routes explicitly
    // rather than reusing `mockFetchByUrl`'s fixed 99/42.
    fetchMock.mockImplementation((input: RequestInfo | URL) => {
      const url = String(input);
      if (/\/api\/stations\/[A-Za-z]{3}\/departures$/.test(url)) {
        return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
      }
      return Promise.resolve(
        new Response(
          JSON.stringify({ journeyId: 199, legId: 1, trackingId: 99, resolutionStatus: 'pending' }),
          { status: 200 },
        ),
      );
    });

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.click(screen.getByRole('button', { name: 'Now' }));
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys', expect.objectContaining({ method: 'POST' }));
    });
    const body = journeyCallBody(fetchMock);
    expect(body.leg.serviceDate).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(() => new Date(body.leg.scheduledDeparture).toISOString()).not.toThrow();
    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/journeys/199');
    });
  });

  // Task 15: the `SegmentedControl`'s second option -- an open time-window
  // search that posts a `window`-mode leg to `POST /Journeys` instead of a
  // `pin`-mode one, via `submitWindow` rather than `submitTrack`.
  describe('window-search mode', () => {
    function switchToWindowMode() {
      fireEvent.click(screen.getByRole('radio', { name: 'Search a time window' }));
    }

    it('defaults to "I know the train" mode, showing the pin-mode fields', () => {
      renderWithMantine(<TrackTrainForm />);
      expect(screen.getByRole('radio', { name: 'I know the train' })).toBeChecked();
      expect(screen.getByLabelText(/Scheduled departure/)).toBeInTheDocument();
      expect(screen.queryByRole('combobox', { name: /^Destination station$/ })).not.toBeInTheDocument();
    });

    // Review §2.1/M23: the toggle swaps ~400px of form with no
    // announcement for a screen-reader user -- a visually-hidden
    // `role="status"` region names the mode so they don't have to tab
    // forward to discover it.
    it('announces the mode switch via a visually-hidden status region', () => {
      renderWithMantine(<TrackTrainForm />);
      expect(screen.getByRole('status')).toHaveTextContent('Showing pick-a-departure search.');

      switchToWindowMode();

      expect(screen.getByRole('status')).toHaveTextContent('Showing time-window search.');
    });

    // Review §2.1/I21: `?mode=window` (wired through `track/page.tsx`) is
    // the one thing that can now send a user straight to this mode -- the
    // toggle used to be the ONLY discovery path, unreachable from anywhere
    // else in the app (not even a bookmark).
    it('starts in window mode when initialMode="window" is passed', () => {
      renderWithMantine(<TrackTrainForm initialMode="window" />);
      expect(screen.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
      expect(screen.getByRole('combobox', { name: /^Destination station$/ })).toBeInTheDocument();
    });

    it('switching to "Search a time window" swaps the pin fields for the window fields', async () => {
      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      // `initialOrigin="WAT"` fires the departures effect on mount -- see
      // the "pre-fills the origin field" test's comment above for why this
      // is awaited before the test ends.
      await waitFor(() => expect(fetch).toHaveBeenCalled());
      switchToWindowMode();

      expect(screen.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
      // The window-mode Destination field is required (no "(optional)"
      // suffix), unlike the pin-mode one -- distinguishes it from the
      // pin-mode field this same query would otherwise also match.
      expect(screen.getByRole('combobox', { name: /^Destination station$/ })).toBeInTheDocument();
      expect(screen.getByLabelText('Earliest departure (optional)')).toBeInTheDocument();
      expect(screen.getByLabelText('Latest departure (optional)')).toBeInTheDocument();
      expect(screen.getByLabelText('Earliest arrival (optional)')).toBeInTheDocument();
      expect(screen.getByLabelText('Latest arrival (optional)')).toBeInTheDocument();
      // The pin-mode-only fields are gone.
      expect(screen.queryByLabelText(/Scheduled departure/)).not.toBeInTheDocument();
      expect(screen.queryByRole('combobox', { name: /Operator/ })).not.toBeInTheDocument();
    });

    it('an all-blank window is blocked client-side, with no network call', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      // See the previous test's comment on why this is awaited.
      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      switchToWindowMode();
      fireEvent.change(screen.getByRole('combobox', { name: /^Destination station$/ }), {
        target: { value: 'RDG' },
      });

      fireEvent.click(screen.getByRole('button', { name: /Search for a train/ }));

      expect(
        screen.getByText('Enter at least one earliest/latest departure or arrival time to search a window.'),
      ).toBeInTheDocument();
      expect(fetchMock).not.toHaveBeenCalledWith('/api/Journeys', expect.anything());
    });

    it('an invalid origin/destination is blocked client-side, with no network call', () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackTrainForm />);
      switchToWindowMode();
      fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });

      fireEvent.click(screen.getByRole('button', { name: /Search for a train/ }));

      expect(
        screen.getByText('Enter a valid origin and destination station before searching.'),
      ).toBeInTheDocument();
      expect(fetchMock).not.toHaveBeenCalledWith('/api/Journeys', expect.anything());
    });

    it('submits a window-mode leg with the entered origin/destination/date/departWindow/arriveWindow, and redirects to /journeys/{journeyId}', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      switchToWindowMode();
      fireEvent.change(screen.getByRole('combobox', { name: /^Destination station$/ }), {
        target: { value: 'RDG' },
      });
      fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });
      fireEvent.change(screen.getByLabelText('Latest arrival (optional)'), { target: { value: '11:30' } });

      fireEvent.click(screen.getByRole('button', { name: /Search for a train/ }));

      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith('/api/Journeys', expect.objectContaining({ method: 'POST' }));
      });
      const body = journeyCallBody(fetchMock);
      expect(body.leg).toEqual({
        mode: 'window',
        originCrs: 'WAT',
        destinationCrs: 'RDG',
        serviceDate: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/),
        departWindow: { after: '09:00', before: null },
        arriveWindow: { after: null, before: '11:30' },
      });
      await waitFor(() => {
        expect(pushMock).toHaveBeenCalledWith('/journeys/99');
      });
    });

    it('an incomplete (half-entered) time field blocks submission even with a real bound entered elsewhere, with no network call', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      // See the earlier "switching to Search a time window" test's comment
      // on why this is awaited.
      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      switchToWindowMode();
      fireEvent.change(screen.getByRole('combobox', { name: /^Destination station$/ }), {
        target: { value: 'RDG' },
      });
      // A real, complete bound -- `windowHasABound` is true, so
      // `handleSubmit`'s own explicit checks all pass and it calls
      // `submitWindow`.
      fireEvent.change(screen.getByLabelText('Latest departure (optional)'), { target: { value: '12:00' } });
      const departFromInput = screen.getByLabelText('Earliest departure (optional)') as HTMLInputElement;
      // Simulate a native `<input type="time">` mid-entry on a DIFFERENT
      // field: `validity.badInput` true -- see `TimeFilterInput`'s own doc
      // comment on why a half-entered time reports this way, and
      // `TrainSearchForm.test.tsx`'s identical technique for its own four
      // `TimeFilterInput` fields.
      Object.defineProperty(departFromInput, 'validity', { configurable: true, get: () => ({ badInput: true }) });
      fireEvent.blur(departFromInput);

      fireEvent.click(screen.getByRole('button', { name: /Search for a train/ }));

      // `canSubmitWindow` (checked inside `submitWindow` itself, not
      // `handleSubmit`'s own gate) is false here because
      // `windowTimesComplete` is false -- so `submitWindow` silently
      // returns before ever calling `fetch`, and no field error is shown
      // either (`handleSubmit` already cleared it before deferring to
      // `submitWindow`).
      expect(fetchMock).not.toHaveBeenCalledWith('/api/Journeys', expect.anything());
      expect(
        screen.queryByText('Enter at least one earliest/latest departure or arrival time to search a window.'),
      ).not.toBeInTheDocument();
    });

    it('on a 401 in window mode, shows the login prompt and preserves the typed window fields', async () => {
      vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response('no session', { status: 401 }))));
      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      switchToWindowMode();
      fireEvent.change(screen.getByRole('combobox', { name: /^Destination station$/ }), {
        target: { value: 'RDG' },
      });
      fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });

      fireEvent.click(screen.getByRole('button', { name: /Search for a train/ }));

      // Review §2.5/M19: the modal's own copy now matches the mode -- a
      // window search hasn't tracked anything yet, so "track this train"
      // was never true here.
      expect(await screen.findByText('Log in to search for a train.')).toBeInTheDocument();
      expect(screen.getByRole('combobox', { name: /^Destination station$/ })).toHaveValue('RDG');
    });

    it('the login-modal copy still says "track this train" in pick mode', async () => {
      vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response('no session', { status: 401 }))));
      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

      expect(await screen.findByText('Log in to track this train.')).toBeInTheDocument();
    });

    // Review §2.2/M14: the ordering check -- previously only PRESENCE was
    // validated, so "earliest after latest" reached the backend unexamined.
    it('rejects a window whose latest departure is before its earliest departure, with no network call', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      switchToWindowMode();
      fireEvent.change(screen.getByRole('combobox', { name: /^Destination station$/ }), {
        target: { value: 'RDG' },
      });
      fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '18:00' } });
      fireEvent.change(screen.getByLabelText('Latest departure (optional)'), { target: { value: '09:00' } });

      fireEvent.click(screen.getByRole('button', { name: /Search for a train/ }));

      expect(screen.getByText('Latest departure must be after earliest departure.')).toBeInTheDocument();
      expect(fetchMock).not.toHaveBeenCalledWith('/api/Journeys', expect.anything());
    });

    it('rejects a window whose latest arrival is before its earliest arrival, with no network call', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      switchToWindowMode();
      fireEvent.change(screen.getByRole('combobox', { name: /^Destination station$/ }), {
        target: { value: 'RDG' },
      });
      fireEvent.change(screen.getByLabelText('Earliest arrival (optional)'), { target: { value: '12:00' } });
      fireEvent.change(screen.getByLabelText('Latest arrival (optional)'), { target: { value: '11:00' } });

      fireEvent.click(screen.getByRole('button', { name: /Search for a train/ }));

      expect(screen.getByText('Latest arrival must be after earliest arrival.')).toBeInTheDocument();
      expect(fetchMock).not.toHaveBeenCalledWith('/api/Journeys', expect.anything());
    });

    // Review §2.2/I17: the labels still say "(optional)" per-field (each
    // one individually is), but the group-level rule -- at least one of
    // the four is required -- is now stated up front instead of only
    // surfacing as a post-submit error.
    it('states the at-least-one-of-four rule up front, not only after a failed submit', () => {
      renderWithMantine(<TrackTrainForm />);
      switchToWindowMode();

      expect(screen.getByText('At least one of the four times below is required to search.')).toBeInTheDocument();
    });

    // Review §2.2/M13: the field shows a real default value, not a grey
    // placeholder indistinguishable from "nothing selected".
    it("seeds the Date field with today's date rather than leaving it an empty placeholder", () => {
      renderWithMantine(<TrackTrainForm />);
      switchToWindowMode();

      // `DatePickerInput`'s labelled control is a `<button>` whose text
      // IS the formatted value (not an `<input value>`) -- see the
      // `DateTimePicker` mock's own comment above for why the pin-mode
      // sibling needs a stand-in but this one, being asserted on its
      // rendered text rather than driven via `fireEvent.change`, does not.
      const dateButton = screen.getByLabelText('Date');
      expect(dateButton).toHaveTextContent(dayjs().format('MMMM D, YYYY'));
      expect(dateButton).not.toHaveTextContent('Today');
    });

    // Review §2.1/I21: the page's own mode-aware intro copy now lives
    // inside this form (see its own comment) rather than as static text
    // owned by the page, so it reacts to the client-side toggle.
    it('switches the intro copy to describe window mode once selected', () => {
      renderWithMantine(<TrackTrainForm />);
      expect(screen.getByText(/Pin a specific train to see its live position/)).toBeInTheDocument();

      switchToWindowMode();

      expect(screen.queryByText(/Pin a specific train to see its live position/)).not.toBeInTheDocument();
      expect(screen.getByText(/Not sure which train yet\?/)).toBeInTheDocument();
    });
  });

  describe('live departures picker', () => {
    const departures = [
      {
        serviceId: 'svc-cancelled',
        operator: 'ZA',
        destinationCrs: 'WAT',
        destinationName: null,
        scheduled: '10:15',
        estimated: 'Cancelled',
        isCancelled: true,
        delayMinutes: 0,
        cancelReason: 'fleet issue',
        delayReason: null,
        skippedStations: [],
        platform: null,
        plannedPlatform: null,
        platformChanged: false,
      },
      {
        serviceId: 'svc-on-time',
        operator: 'SW',
        destinationCrs: 'BSK',
        destinationName: 'Basingstoke',
        scheduled: '10:40',
        estimated: 'On time',
        isCancelled: false,
        delayMinutes: 0,
        cancelReason: null,
        delayReason: null,
        skippedStations: [],
        platform: '4',
        plannedPlatform: '4',
        platformChanged: false,
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

    // 2026-09-22 UX review follow-up (item 5): the live picker used to show
    // only the raw destination CRS code -- `render::station_departure_json`
    // now resolves it server-side via a batched `stations` lookup, and this
    // picker renders it through the shared `stationLabel` convention.
    it('renders a resolved destination name alongside its code, falling back to the bare code when unresolved', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      expect(await screen.findByText(/Basingstoke \(BSK\)/)).toBeInTheDocument();
      // The cancelled row's destination (WAT) has no resolved name in this
      // fixture -- falls back to the bare code, same convention as
      // elsewhere in this app.
      expect(screen.getByText(/10:15 · WAT/)).toBeInTheDocument();
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

    it('renders the picked row\'s platform badge alongside its status badge', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

      // `departures[1]` ("svc-on-time") is fixture-seeded with platform "4"
      // and no change -- see this describe block's own `departures` const.
      expect(await screen.findByText('Platform 4')).toBeInTheDocument();
      // The cancelled row has no platform in the fixture -- no badge at
      // all for it, same "not known, don't fabricate" posture as
      // `PlatformBadge` itself.
      expect(screen.queryByText(/Platform \(?[^4]/)).not.toBeInTheDocument();
    });

    it('picking a row carries its platform snapshot through to the pin submission', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      fireEvent.click(await screen.findByRole('button', { name: /10:40/ }));
      fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

      // Integration note (2026-09-22): the platform snapshot used to ride on
      // the legacy flat `POST /Train/track` body as
      // `platform`/`planned_platform`. `POST /Journeys` replaced that route,
      // so it now travels inside the pin-mode LEG in camelCase -- same two
      // values, same meaning, new envelope.
      await waitFor(() => {
        const body = journeyCallBody(fetchMock);
        expect(body.leg.platform).toBe('4');
        expect(body.leg.plannedPlatform).toBe('4');
      });
    });

    it('picking a departure-board row with skipped stations carries skippedStations through to the submitted leg', async () => {
      // Regression for Finding I2: `pickDeparture` captures
      // `row.skippedStations` into state, but the submitted `POST
      // /api/Journeys` body used to never include it at all -- the value
      // was captured then silently discarded. Confirm it now reaches the
      // wire.
      const rowsWithSkips = [
        {
          serviceId: 'svc-skips',
          operator: 'SW',
          destinationCrs: 'BSK',
          scheduled: '10:40',
          estimated: 'On time',
          isCancelled: false,
          delayMinutes: 0,
          cancelReason: null,
          delayReason: null,
          skippedStations: ['CLJ', 'WOK'],
        },
      ];
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(rowsWithSkips), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      const onTimeRow = await screen.findByRole('button', { name: /10:40/ });
      fireEvent.click(onTimeRow);
      fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith('/api/Journeys', expect.objectContaining({ method: 'POST' }));
      });
      const body = journeyCallBody(fetchMock);
      expect(body.leg.skippedStations).toEqual(['CLJ', 'WOK']);
    });

    // Regression coverage for the LDBWS sibling of the CIF post-midnight
    // day-offset bug (see the `pickCifDeparture`/`ScheduleDepartureRow`
    // tests further below): `DepartureRow.scheduled` is a bare "HH:MM" with
    // no date or day-offset field at all, and `pickDeparture` used to
    // unconditionally combine it with TODAY's date -- wrong whenever the
    // picked row is actually tomorrow relative to when the live board was
    // viewed (e.g. viewing the board at 23:50 and picking a "00:07" row,
    // a real, near-term, 17-minutes-away departure). `resolveLdbwsDepartureDate`
    // fixes this by comparing against real wall-clock "now" (`dayjs()`,
    // pinned via `vi.setSystemTime` below), not the typed `scheduledDeparture`
    // field -- see that function's own doc comment for the exact threshold
    // and why.
    describe('LDBWS midnight-wraparound day resolution', () => {
      function ldbwsRow(scheduled: string) {
        return [
          {
            serviceId: 'svc-midnight',
            operator: 'SW',
            destinationCrs: 'BSK',
            scheduled,
            estimated: 'On time',
            isCancelled: false,
            delayMinutes: 0,
            cancelReason: null,
            delayReason: null,
            skippedStations: [],
            platform: null,
            plannedPlatform: null,
            platformChanged: false,
          },
        ];
      }

      it('a normal same-day pick close to "now" stays on today\'s date', async () => {
        vi.setSystemTime(new Date('2026-09-05T10:00:00.000Z'));
        const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(ldbwsRow('10:15')), { status: 200 }) });
        vi.stubGlobal('fetch', fetchMock);

        renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
        const row = await screen.findByRole('button', { name: /10:15/ });
        const today = dayjs().format('YYYY-MM-DD');
        fireEvent.click(row);

        const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
        expect(picker.value).toBe(`${today} 10:15:00`);
      });

      it('the concrete failure case -- now 23:50, scheduled 00:07 -- resolves to tomorrow, not today', async () => {
        vi.setSystemTime(new Date('2026-09-05T23:50:00.000Z'));
        const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(ldbwsRow('00:07')), { status: 200 }) });
        vi.stubGlobal('fetch', fetchMock);

        renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
        // Also proves the `matchesScheduledDeparture` LDBWS exposure fix:
        // without it, this row (implicitly "today 00:07" against a
        // default `scheduledDeparture` of "today 23:50") would read as
        // already-passed and never even appear here to click.
        const row = await screen.findByRole('button', { name: /00:07/ });
        const tomorrow = dayjs().add(1, 'day').format('YYYY-MM-DD');
        fireEvent.click(row);

        const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
        expect(picker.value).toBe(`${tomorrow} 00:07:00`);
      });

      // Boundary around the chosen threshold (4 hours -- comfortably more
      // than Darwin's ~2-hour default look-ahead window, see
      // `LDBWS_PAST_THRESHOLD_HOURS`'s own doc comment in TrackTrainForm.tsx
      // for the full reasoning): "now" is pinned at 10:00, so a same-day
      // combination of "06:01" is 3h59m in the past (just inside the
      // threshold -- not corrected), while "05:59" is 4h01m in the past
      // (just outside it -- corrected to tomorrow). Deliberately 1 minute
      // either side of the exact 4h line, not AT it: `vi.useFakeTimers`'s
      // `shouldAdvanceTime` (needed elsewhere in this file for `waitFor`/
      // `findBy*` to work) lets real wall-clock time tick the fake clock
      // forward by a few milliseconds across each `await` below, which
      // would otherwise flip a test sitting exactly on the boundary; a
      // one-minute margin on each side comfortably absorbs that without
      // weakening what the pair proves -- a specific, reasoned cutover, not
      // an arbitrary/off-by-one one.
      it('a same-day combination just inside the threshold (3h59m before "now") is NOT corrected', async () => {
        vi.setSystemTime(new Date('2026-09-05T10:00:00.000Z'));
        const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(ldbwsRow('06:01')), { status: 200 }) });
        vi.stubGlobal('fetch', fetchMock);

        renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
        // Widen `scheduledDeparture` (defaults to mount-time "now", 10:00)
        // back to local midnight -- purely so `matchesScheduledDeparture`'s
        // OWN "already passed relative to what's typed" filter doesn't hide
        // this deliberately-in-the-past-today row before the day-RESOLUTION
        // logic under test even gets a chance to run; a live board would
        // never actually show an already-departed row like this one, so
        // this step is test scaffolding, not something real usage needs.
        fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
          target: { value: `${dayjs().format('YYYY-MM-DD')} 00:00:00` },
        });
        const row = await screen.findByRole('button', { name: /06:01/ });
        const today = dayjs().format('YYYY-MM-DD');
        fireEvent.click(row);

        const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
        expect(picker.value).toBe(`${today} 06:01:00`);
      });

      it('a same-day combination just outside the threshold (4h01m before "now") IS corrected to tomorrow', async () => {
        vi.setSystemTime(new Date('2026-09-05T10:00:00.000Z'));
        const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(ldbwsRow('05:59')), { status: 200 }) });
        vi.stubGlobal('fetch', fetchMock);

        renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
        // See the previous test's comment on why this is widened to
        // midnight first -- same reasoning.
        fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
          target: { value: `${dayjs().format('YYYY-MM-DD')} 00:00:00` },
        });
        const row = await screen.findByRole('button', { name: /05:59/ });
        const tomorrow = dayjs().add(1, 'day').format('YYYY-MM-DD');
        fireEvent.click(row);

        const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
        expect(picker.value).toBe(`${tomorrow} 05:59:00`);
      });
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

    const scheduleDepartures: {
      uid: string;
      scheduled: string;
      dayOffset: number;
      destinationCrs: string | null;
      destinationName: string | null;
    }[] = [
      { uid: 'C11052', scheduled: '08:22', dayOffset: 0, destinationCrs: 'CRE', destinationName: 'Crewe' },
      { uid: 'C99999', scheduled: '09:00', dayOffset: 0, destinationCrs: null, destinationName: null },
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
      // 2026-09-22 UX review follow-up (item 5): resolved via the same
      // server-side batched lookup as the LDBWS branch above.
      expect(screen.getByText(/08:22 · Crewe \(CRE\)/)).toBeInTheDocument();
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

    it('a post-midnight CIF row (dayOffset > 0) stays visible late at night and derives the correct next-day service_date on pick', async () => {
      // Regression coverage for the exact bug this task fixes: a real
      // overnight CIF schedule's post-midnight calling point (e.g. the
      // live-confirmed c2c Barking 00:07, `schedule_query::resolve`'s own
      // `f49687_raw` fixture) is genuinely TOMORROW relative to when the
      // search ran, not "today" -- combining its bare `"00:07"` with the
      // browser's current date would create a pin dated the wrong calendar
      // day.
      const postMidnight = [{ uid: 'F49687', scheduled: '00:07', dayOffset: 1, destinationCrs: 'SNF' }];
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response(JSON.stringify(postMidnight), { status: 200 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      const row = await screen.findByRole('button', { name: /00:07/ });

      // Simulate a late-night search first: without `dayOffset` factored
      // into `matchesScheduledDeparture`, comparing the row's bare time
      // against TODAY's date would read "today 00:07" as already-passed
      // relative to "today 23:50" and silently hide the row from the
      // picker -- even though it is really tomorrow and very much still in
      // the future. This row staying visible here is itself part of the
      // regression coverage, not just setup for the click below.
      fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
        target: { value: '2026-09-05 23:50:00' },
      });
      expect(screen.getByRole('button', { name: /00:07/ })).toBeInTheDocument();

      // The same `dayjs()` read `pickCifDeparture` itself makes, at the
      // moment of picking -- not a parse of the typed '2026-09-05' value
      // above, which only drives the filter check, not the pick.
      const tomorrow = dayjs().add(1, 'day').format('YYYY-MM-DD');
      fireEvent.click(row);

      const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
      expect(picker.value).toBe(`${tomorrow} 00:07:00`);

      fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));
      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith('/api/Journeys', expect.objectContaining({ method: 'POST' }));
      });
      const body = journeyCallBody(fetchMock);
      expect(body.leg.serviceDate).toBe(tomorrow);
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

    it('renders a "View live status" link on each CIF row, pointing at /train/{uid}/{today}', async () => {
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      await screen.findByRole('button', { name: /08:22/ });
      // Reuses this file's existing technique (see the "clicking a
      // non-cancelled row" test above) for reading "today" deterministically
      // via the same `dayjs()` call the component itself makes.
      const today = dayjs().format('YYYY-MM-DD');

      const links = screen.getAllByRole('link', { name: 'View live status' });
      expect(links).toHaveLength(2);
      expect(links[0]).toHaveAttribute('href', `/train/C11052/${today}`);
      expect(links[1]).toHaveAttribute('href', `/train/C99999/${today}`);
    });

    it('does not render a "View live status" link on LDBWS rows -- DepartureRow carries no train UID', async () => {
      const fetchMock = mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      await screen.findByRole('button', { name: /10:40/ });

      expect(screen.queryByRole('link', { name: 'View live status' })).not.toBeInTheDocument();
    });

    it('clicking a CIF row\'s "View live status" link does not also select the row for tracking', async () => {
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      await screen.findByRole('button', { name: /08:22/ });
      const today = dayjs().format('YYYY-MM-DD');

      // Two CIF fixture rows both render this link -- the first is C11052's.
      const link = screen.getAllByRole('link', { name: 'View live status' })[0];
      expect(link).toHaveAttribute('href', `/train/C11052/${today}`);

      fireEvent.click(link);

      // pickCifDeparture would have filled Destination/Scheduled-departure
      // from this row -- it must not have run.
      expect(screen.getByRole('combobox', { name: /Destination station/ })).toHaveValue('');
      const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
      expect(picker.value).toBe(dayjs().format('YYYY-MM-DD HH:mm:ss'));
    });

    it('pressing Enter on a focused "View live status" link does not also select the row for tracking', async () => {
      const fetchMock = mockFetchByUrl({
        departures: () => new Response('not found', { status: 404 }),
        scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
      });
      vi.stubGlobal('fetch', fetchMock);

      renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
      await screen.findByRole('button', { name: /08:22/ });

      const link = screen.getAllByRole('link', { name: 'View live status' })[0];
      link.focus();
      fireEvent.keyDown(link, { key: 'Enter' });

      // The row's own onKeyDown (which calls pickCifDeparture on Enter) must
      // not have fired via bubbling from the nested link.
      expect(screen.getByRole('combobox', { name: /Destination station/ })).toHaveValue('');
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

    // Regression guard for the picker being hard-clipped at a fixed height.
    // Both row-list branches used to sit inside a `<ScrollArea mah={220}
    // offsetScrollbars>`, whose root is `overflow: hidden` while its
    // viewport is `height: 100%`; against a root whose own `height` stays
    // `auto` that percentage resolves to `auto`, so the viewport never
    // overflowed itself (nothing scrolled) and the root simply clipped
    // everything past 220px. At ~30px of pitch a row (a `size="sm"` line
    // plus the `Stack`'s `xs` gap) that landed after about seven of
    // them, and -- because these rows are `role="button"` pickers, not
    // text -- every departure past the cap was silently UNSELECTABLE: not
    // reachable by pointer, not by wheel (there was no scroller to spin),
    // and by keyboard only into a dead end, since revealing a focused
    // descendant of an `overflow: hidden` box scrolls it to an offset the
    // user then has no gesture to undo. jsdom does no layout, so these
    // assert the *structure* that caused it plus the behaviour it broke.
    //
    // Note the structural guard also rejects `ScrollArea.Autosize` -- the
    // component that *would* cap the height correctly. Deliberate, matching
    // `IncidentSearchForm.test.tsx`/`TrainSearchForm.test.tsx`'s identical
    // guards: the choice here is "no nested scroller at all, the page
    // scrolls", and `pickerContent`'s own doc comment records why.
    describe('every picker row stays reachable, however many there are', () => {
      /** Deliberately 10 rows -- the number both sources actually publish
       * (see `pickerContent`'s own doc comment), and comfortably past the
       * ~7 that used to fit inside the removed 220px cap. '10:55', the
       * last, is the one that mattered: under the old `ScrollArea` it was
       * rendered and exposed to the a11y tree, and yet impossible to
       * select. */
      const MANY_LDBWS = Array.from({ length: 10 }, (_, i) => ({
        serviceId: `svc-${i}`,
        operator: 'SW',
        destinationCrs: 'BSK',
        scheduled: `10:${String(10 + i * 5).padStart(2, '0')}`,
        estimated: 'On time',
        isCancelled: false,
        delayMinutes: 0,
        cancelReason: null,
        delayReason: null,
        skippedStations: [],
        platform: null,
        plannedPlatform: null,
        platformChanged: false,
      }));
      const MANY_CIF = Array.from({ length: 10 }, (_, i) => ({
        uid: `C2000${i}`,
        scheduled: `10:${String(10 + i * 5).padStart(2, '0')}`,
        dayOffset: 0,
        destinationCrs: 'CRE',
      }));

      /** Walks the row list plus every ancestor up to (and including) the
       * form, asserting none of them is a clipped-but-non-scrolling box. A
       * clip anywhere on that chain hides rows just as effectively as one
       * on the list itself. */
      function expectNoClippingAncestor() {
        const list = document.querySelector('[data-departure-picker-rows]');
        expect(list).not.toBeNull();
        const form = (list as HTMLElement).closest('form');
        expect(form).not.toBeNull();
        for (
          let node: HTMLElement | null = list as HTMLElement;
          node !== null;
          node = node === form ? null : (node.parentElement as HTMLElement | null)
        ) {
          // Mantine's own scroll viewport, whatever set it up.
          expect(node.hasAttribute('data-scrollarea-viewport')).toBe(false);
          // Mantine resolves a non-responsive `h`/`mah` style prop straight
          // into an inline `height`/`max-height` (`parse-style-props.mjs`),
          // so reading those back off `style` is enough -- no computed
          // style, no layout, which is just as well under jsdom. Verified
          // against the rendered DOM: a `<ScrollArea mah={220}>` root
          // carried `max-height: calc(13.75rem * var(--mantine-scale))`.
          expect(node.style.maxHeight).toBe('');
          expect(node.style.height).toBe('');
          // Only catches a hand-written inline clip -- Mantine's own
          // `overflow: hidden` arrives via the `.m_d57069b5` class, which
          // the `data-scrollarea-viewport` check above is what covers.
          // Known gap, accepted: a RESPONSIVE `mah={{ base: 220 }}` compiles
          // to a generated stylesheet rule rather than an inline style, as
          // would a clip arriving via a CSS module or a global class, and
          // neither would be seen here. The `data-scrollarea-viewport` check
          // still catches every `ScrollArea`-shaped reintroduction, which is
          // the realistic one.
          expect(node.style.overflow).not.toBe('hidden');
          expect(node.style.overflowY).not.toBe('hidden');
        }
      }

      it('LDBWS: the last row is in the same in-flow list as the first, under no clipping ancestor', async () => {
        vi.setSystemTime(new Date('2026-09-05T09:00:00.000Z'));
        vi.stubGlobal(
          'fetch',
          mockFetchByUrl({ departures: () => new Response(JSON.stringify(MANY_LDBWS), { status: 200 }) }),
        );
        renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

        const first = await screen.findByRole('button', { name: /10:10/ });
        const last = screen.getByRole('button', { name: /10:55/ });
        const list = document.querySelector('[data-departure-picker-rows]') as HTMLElement;
        expect(list.contains(first)).toBe(true);
        expect(list.contains(last)).toBe(true);
        expectNoClippingAncestor();
      });

      // The two "still selectable" cases below are a contract, not a
      // reproduction: jsdom lays nothing out, so a synthetic click or
      // keydown reaches a clipped node just as happily as a visible one --
      // both of these DO pass against the old `ScrollArea`. What catches
      // the regression is `expectNoClippingAncestor` above; these pin down
      // what the structure is protecting, and would catch a "fix" that
      // removed the clip by making the rows inert instead.
      it('LDBWS: a row well past the old 220px cap is still selectable by pointer', async () => {
        vi.setSystemTime(new Date('2026-09-05T09:00:00.000Z'));
        vi.stubGlobal(
          'fetch',
          mockFetchByUrl({ departures: () => new Response(JSON.stringify(MANY_LDBWS), { status: 200 }) }),
        );
        renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

        // The 10th row -- roughly 280px down the list, well past where
        // the old 220px clip cut it off and made it unclickable.
        const last = await screen.findByRole('button', { name: /10:55/ });
        const today = dayjs().format('YYYY-MM-DD');
        fireEvent.click(last);

        expect((screen.getByLabelText(/Scheduled departure/) as HTMLInputElement).value).toBe(
          `${today} 10:55:00`,
        );
      });

      it('LDBWS: that same row is still selectable by keyboard (Enter on the focused row)', async () => {
        vi.setSystemTime(new Date('2026-09-05T09:00:00.000Z'));
        vi.stubGlobal(
          'fetch',
          mockFetchByUrl({ departures: () => new Response(JSON.stringify(MANY_LDBWS), { status: 200 }) }),
        );
        renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

        const last = await screen.findByRole('button', { name: /10:55/ });
        // A keyboard user reaches it by tabbing: every row is its own focus
        // stop, so assert this one really is one rather than assuming it.
        expect(last).toHaveAttribute('tabindex', '0');
        last.focus();
        expect(document.activeElement).toBe(last);
        const today = dayjs().format('YYYY-MM-DD');
        fireEvent.keyDown(last, { key: 'Enter' });

        expect((screen.getByLabelText(/Scheduled departure/) as HTMLInputElement).value).toBe(
          `${today} 10:55:00`,
        );
      });

      it('CIF: the last row is in the same in-flow list, under no clipping ancestor, and still selectable', async () => {
        vi.setSystemTime(new Date('2026-09-05T09:00:00.000Z'));
        vi.stubGlobal(
          'fetch',
          mockFetchByUrl({
            departures: () => new Response('not found', { status: 404 }),
            scheduleDepartures: () => new Response(JSON.stringify(MANY_CIF), { status: 200 }),
          }),
        );
        renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

        const first = await screen.findByRole('button', { name: /10:10/ });
        const last = screen.getByRole('button', { name: /10:55/ });
        const list = document.querySelector('[data-departure-picker-rows]') as HTMLElement;
        expect(list.contains(first)).toBe(true);
        expect(list.contains(last)).toBe(true);
        expectNoClippingAncestor();

        const today = dayjs().format('YYYY-MM-DD');
        fireEvent.click(last);
        expect((screen.getByLabelText(/Scheduled departure/) as HTMLInputElement).value).toBe(
          `${today} 10:55:00`,
        );
      });
    });
  });

  // Shared-groups follow-up: the "Personal or one of your groups?" prompt.
  describe('group-share destination prompt', () => {
    const GROUPS_FIXTURE: GroupSummary[] = [
      { id: 'grp-1', name: 'Family', role: 'owner', memberCount: 3 },
      { id: 'grp-2', name: 'Commuters', role: 'member', memberCount: 5 },
    ];

    it('opens the destination prompt instead of submitting immediately when the user has at least one group', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithGroups(<TrackTrainForm initialOrigin="WAT" />, GROUPS_FIXTURE);

      fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

      expect((await screen.findAllByLabelText('Track into')).length).toBeGreaterThan(0);
      expect(fetchMock).not.toHaveBeenCalledWith('/api/Journeys', expect.anything());
      expect(pushMock).not.toHaveBeenCalled();
    });

    it('confirming with the default "Personal" selection submits exactly as before, with no group-share call', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithGroups(<TrackTrainForm initialOrigin="WAT" />, GROUPS_FIXTURE);

      fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));
      await screen.findAllByLabelText('Track into');
      fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith('/api/Journeys', expect.objectContaining({ method: 'POST' }));
      });
      await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/journeys/99'));
      expect(fetchMock).not.toHaveBeenCalledWith(expect.stringContaining('/groups/'), expect.anything());
    });

    it('choosing a group submits the pin, then shares it into that group, then navigates', async () => {
      const fetchMock = vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        if (/\/api\/stations\/[A-Za-z]{3}\/departures$/.test(url)) {
          return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
        }
        if (url === '/api/Journeys') {
          return Promise.resolve(
            new Response(
              JSON.stringify({ journeyId: 99, legId: 1, trackingId: 42, resolutionStatus: 'pending' }),
              { status: 200 },
            ),
          );
        }
        if (url === '/api/groups/grp-1/trains') return Promise.resolve(new Response(null, { status: 204 }));
        throw new Error(`unexpected fetch for ${url}`);
      });
      vi.stubGlobal('fetch', fetchMock);
      renderWithGroups(<TrackTrainForm initialOrigin="WAT" />, GROUPS_FIXTURE);

      fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));
      const [select] = await screen.findAllByLabelText('Track into');
      fireEvent.click(select);
      fireEvent.click(await screen.findByText('Family'));
      fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith('/api/Journeys', expect.objectContaining({ method: 'POST' }));
      });
      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith(
          '/api/groups/grp-1/trains',
          expect.objectContaining({ method: 'POST', body: JSON.stringify({ trainSubscriptionId: 42 }) }),
        );
      });
      await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/journeys/99'));

      // The share call must happen strictly after the track call -- the
      // share body needs the real trackingId the track call returns.
      const urls = fetchMock.mock.calls.map((args: unknown[]) => String(args[0]));
      expect(urls.indexOf('/api/Journeys')).toBeLessThan(urls.indexOf('/api/groups/grp-1/trains'));
    });

    it('a group-share failure still redirects, without showing a track-failed error', async () => {
      const fetchMock = vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        if (/\/api\/stations\/[A-Za-z]{3}\/departures$/.test(url)) {
          return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
        }
        if (url === '/api/Journeys') {
          return Promise.resolve(
            new Response(
              JSON.stringify({ journeyId: 99, legId: 1, trackingId: 42, resolutionStatus: 'pending' }),
              { status: 200 },
            ),
          );
        }
        if (url === '/api/groups/grp-1/trains') return Promise.reject(new Error('network blip'));
        throw new Error(`unexpected fetch for ${url}`);
      });
      vi.stubGlobal('fetch', fetchMock);
      renderWithGroups(<TrackTrainForm initialOrigin="WAT" />, GROUPS_FIXTURE);

      fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));
      const [select] = await screen.findAllByLabelText('Track into');
      fireEvent.click(select);
      fireEvent.click(await screen.findByText('Family'));
      fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

      await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/journeys/99'));
      expect(screen.queryByText("Couldn't create the tracking pin. Try again.")).not.toBeInTheDocument();
    });

    it('does not show the prompt at all when the user has zero groups', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithGroups(<TrackTrainForm initialOrigin="WAT" />, []);

      fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

      await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/journeys/99'));
      expect(screen.queryAllByLabelText('Track into')).toHaveLength(0);
    });
  });
  // 2026-09-22 UX review, I9/P4: this toggle passed only
  // value/onChange/data -- no heading, legend or aria-label -- and it is
  // the ONLY discovery path for window mode.
  it('names the pick-mode / window-mode radiogroup', () => {
    renderWithMantine(<TrackTrainForm />);
    expect(
      screen.getByRole('radiogroup', { name: 'How do you want to find the train?' }),
    ).toBeInTheDocument();
    expect(screen.getByText('How do you want to find the train?')).toBeInTheDocument();
  });
});
