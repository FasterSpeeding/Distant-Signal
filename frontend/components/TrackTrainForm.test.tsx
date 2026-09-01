import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackTrainForm } from './TrackTrainForm';

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
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('pre-fills the origin field from initialOrigin', () => {
    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    expect(screen.getByLabelText(/Origin CRS code/)).toHaveValue('WAT');
  });

  it('disables submit until the origin is a valid 3-letter code and a departure is picked', () => {
    renderWithMantine(<TrackTrainForm />);
    expect(screen.getByRole('button', { name: /Track this train/ })).toBeDisabled();
  });

  it('shows a field error for a non-3-letter origin code', () => {
    renderWithMantine(<TrackTrainForm />);
    fireEvent.change(screen.getByLabelText(/Origin CRS code/), { target: { value: 'WATERLOO' } });
    expect(screen.getByText('Must be a 3-letter CRS code')).toBeInTheDocument();
  });

  it('on success, POSTs to /api/Train/track and redirects to /train/by-id/{trackingId}', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response(JSON.stringify({ trackingId: 42, resolutionStatus: 'pending' }), { status: 200 }),
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

  it('on a 401, shows the login prompt and preserves the typed field values', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Destination CRS code/), { target: { value: 'WOK' } });
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to track this train' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login?return_to=%2Ftrack');
    // Unlike PinToggle's toggle-and-forget click, the form's own input
    // must survive a 401 -- Decision 4's explicit "preserve typed values"
    // call.
    expect(screen.getByLabelText(/Origin CRS code/)).toHaveValue('WAT');
    expect(screen.getByLabelText(/Destination CRS code/)).toHaveValue('WOK');
  });

  it('on a 400, shows the server error message inline', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response('scheduled_departure is too far in the past to track', { status: 400 }),
    );

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    expect(await screen.findByText('scheduled_departure is too far in the past to track')).toBeInTheDocument();
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

  it('derives service_date from the picker\'s local wall-clock date, not the UTC date', async () => {
    // A local time just after midnight, near a UTC day boundary (e.g.
    // during BST, UTC+1): the naive `new Date(...).toISOString().slice(0,
    // 10)` approach would roll this back to '2026-08-28', the WRONG
    // calendar date the user actually picked.
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response(JSON.stringify({ trackingId: 7, resolutionStatus: 'pending' }), { status: 200 }),
    );

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-29 00:30:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalled();
    });
    const [, init] = fetchMock.mock.calls[0];
    const body = JSON.parse(init!.body as string);
    expect(body.service_date).toBe('2026-08-29');
  });
});
