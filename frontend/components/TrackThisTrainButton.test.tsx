import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackThisTrainButton } from './TrackThisTrainButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

/** Routes a mocked `fetch` by URL, the same shape
 * `TrackTrainForm.test.tsx`'s own `mockFetchByUrl` helper uses: the
 * by-uid track call and the ticket-attach follow-up are configured
 * independently so a test can make one fail without the other. */
function mockFetchByUrl(
  options: { track?: () => Response; attach?: () => Response | Promise<Response> } = {},
) {
  const {
    track = () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
    attach = () => new Response(JSON.stringify({ ticketId: 7, trackedTrainId: 42 }), { status: 200 }),
  } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (/\/api\/Train\/tickets\/\d+\/attach$/.test(url)) return Promise.resolve(attach());
    if (/\/api\/Train\/by-uid\/.+\/track$/.test(url)) return Promise.resolve(track());
    throw new Error(`unexpected fetch for ${url}`);
  });
}

describe('TrackThisTrainButton', () => {
  beforeEach(() => {
    pushMock.mockClear();
  });

  it('POSTs to the by-uid track route with the uid and date from its props', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/by-uid/C11052/2026-09-07/track',
        expect.objectContaining({ method: 'POST' }),
      );
    });
  });

  it('percent-encodes a path-like uid', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052/../mine" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/by-uid/C11052%2F..%2Fmine/2026-09-07/track',
        expect.objectContaining({ method: 'POST' }),
      );
    });
  });

  it('navigates to the new tracking id on success', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('makes no ticket-attach call when attachTicketId is absent', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalled());
    const attachCalls = fetchMock.mock.calls.filter((args: unknown[]) =>
      String(args[0]).includes('/attach'),
    );
    expect(attachCalls).toHaveLength(0);
  });

  it('attaches the ticket after a successful track when attachTicketId is given', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/tickets/7/attach',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ trackingId: 42 }),
        }),
      );
    });
  });

  // The parity requirement's real substance: a failed attach must not block
  // navigation, exactly as TrackTrainForm.tsx:321-335 already behaves.
  it('still navigates when the ticket-attach follow-up rejects', async () => {
    const fetchMock = mockFetchByUrl({ attach: () => Promise.reject(new Error('network blip')) });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('still navigates when the ticket-attach follow-up returns a 409', async () => {
    const fetchMock = mockFetchByUrl({
      attach: () => new Response('ticket is already attached to a tracked train', { status: 409 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('opens the login prompt and does not navigate on a 401', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ track: () => new Response('unauthorized', { status: 401 }) }));
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    expect(await screen.findByText('Log in to track this train.')).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('shows an error and does not navigate on a 500', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ track: () => new Response('boom', { status: 500 }) }));
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    expect(await screen.findByText("Couldn't track this train. Try again.")).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  // The concurrent-double-submit guard Task 8's in-function fix cannot
  // close on its own -- asserted, not assumed.
  it('disables itself while a request is in flight', async () => {
    let resolveTrack: (value: Response) => void = () => {};
    const pending = new Promise<Response>((resolve) => {
      resolveTrack = resolve;
    });
    vi.stubGlobal(
      'fetch',
      vi.fn((input: RequestInfo | URL) => {
        if (/\/track$/.test(String(input))) return pending;
        throw new Error(`unexpected fetch for ${String(input)}`);
      }),
    );
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    const button = screen.getByRole('button', { name: 'Track this train' });
    fireEvent.click(button);

    await waitFor(() => expect(screen.getByRole('button', { name: 'Tracking…' })).toBeDisabled());

    resolveTrack(new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }));
    await waitFor(() => expect(pushMock).toHaveBeenCalled());
  });
});
