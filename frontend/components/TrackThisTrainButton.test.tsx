import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackThisTrainButton } from './TrackThisTrainButton';

/** Flushes the microtask (and, via the macrotask boundary, guaranteed to be
 * AFTER every pending microtask) queue -- needed anywhere a test needs
 * `useGroupSummaries`'s mount-time `GET /api/groups` fetch (and its
 * `.then(response => response.json()).then(setGroups)` chain) to have fully
 * settled BEFORE the test's next `fireEvent.click`, since that click's own
 * behavior branches on whether `groups` has loaded yet. A plain
 * `await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/groups'))`
 * is not sufficient here: that call happens synchronously at mount, so the
 * check passes before the async `.then` chain (and the resulting state
 * update) has actually run. */
async function flushGroupsFetch() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

/** Routes a mocked `fetch` by URL, the same shape
 * `TrackTrainForm.test.tsx`'s own `mockFetchByUrl` helper uses: the
 * by-uid track call, the ticket-attach follow-up, and the shared-groups
 * `GET /api/groups` prefetch are all configured independently so a test can
 * make any one of them fail/succeed without the others. `groups` defaults
 * to an empty-array 200 -- the zero-groups case every pre-existing test in
 * this file exercises -- so only tests that specifically cover the
 * has-groups prompt need to override it. */
function mockFetchByUrl(
  options: {
    track?: () => Response;
    attach?: () => Response | Promise<Response>;
    groups?: () => Response;
  } = {},
) {
  const {
    track = () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
    attach = () => new Response(JSON.stringify({ ticketId: 7, trackedTrainId: 42 }), { status: 200 }),
    groups = () => new Response(JSON.stringify([]), { status: 200 }),
  } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url === '/api/groups') return Promise.resolve(groups());
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
        const url = String(input);
        if (url === '/api/groups') return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
        if (/\/track$/.test(url)) return pending;
        throw new Error(`unexpected fetch for ${url}`);
      }),
    );
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    const button = screen.getByRole('button', { name: 'Track this train' });
    fireEvent.click(button);

    await waitFor(() => expect(screen.getByRole('button', { name: 'Tracking…' })).toBeDisabled());

    resolveTrack(new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }));
    await waitFor(() => expect(pushMock).toHaveBeenCalled());
  });

  // Shared-groups follow-up: the "Personal or one of your groups?" prompt.
  describe('group-share destination prompt', () => {
    const GROUPS_FIXTURE = [
      { id: 'grp-1', name: 'Family', role: 'owner', memberCount: 3 },
      { id: 'grp-2', name: 'Commuters', role: 'member', memberCount: 5 },
    ];

    function groupsResponse(groups: unknown[] = GROUPS_FIXTURE) {
      return () => new Response(JSON.stringify(groups), { status: 200 });
    }

    it('opens the destination prompt instead of tracking immediately when the user has at least one group', async () => {
      const fetchMock = mockFetchByUrl({ groups: groupsResponse() });
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);
      await flushGroupsFetch();

      fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

      expect((await screen.findAllByLabelText('Track into')).length).toBeGreaterThan(0);
      const trackCalls = fetchMock.mock.calls.filter((args: unknown[]) => /\/track$/.test(String(args[0])));
      expect(trackCalls).toHaveLength(0);
      expect(pushMock).not.toHaveBeenCalled();
    });

    it('confirming with the default "Personal" selection tracks privately, with no group-share call', async () => {
      const fetchMock = mockFetchByUrl({ groups: groupsResponse() });
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);
      await flushGroupsFetch();

      fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));
      await screen.findAllByLabelText('Track into');
      fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith(
          '/api/Train/by-uid/C11052/2026-09-07/track',
          expect.objectContaining({ method: 'POST' }),
        );
      });
      await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
      expect(fetchMock).not.toHaveBeenCalledWith(expect.stringContaining('/groups/'), expect.anything());
    });

    it('choosing a group tracks the train, then shares it into that group, then navigates', async () => {
      const fetchMock = mockFetchByUrl({ groups: groupsResponse() });
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);
      await flushGroupsFetch();

      fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));
      const [select] = await screen.findAllByLabelText('Track into');
      fireEvent.click(select);
      fireEvent.click(await screen.findByText('Family'));
      fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith(
          '/api/Train/by-uid/C11052/2026-09-07/track',
          expect.objectContaining({ method: 'POST' }),
        );
      });
      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith(
          '/api/groups/grp-1/trains',
          expect.objectContaining({ method: 'POST', body: JSON.stringify({ trainSubscriptionId: 42 }) }),
        );
      });
      await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));

      // The share call must happen strictly after the track call, not
      // before/concurrently -- the share body needs the real trackingId the
      // track call returns.
      const urls = fetchMock.mock.calls.map((args: unknown[]) => String(args[0]));
      expect(urls.indexOf('/api/Train/by-uid/C11052/2026-09-07/track')).toBeLessThan(
        urls.indexOf('/api/groups/grp-1/trains'),
      );
    });

    it('a group-share failure still navigates, without showing a track-failed error', async () => {
      const fetchMock = vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        if (url === '/api/groups') return Promise.resolve(new Response(JSON.stringify(GROUPS_FIXTURE), { status: 200 }));
        if (/\/track$/.test(url)) {
          return Promise.resolve(new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }));
        }
        if (/\/groups\/grp-1\/trains$/.test(url)) return Promise.reject(new Error('network blip'));
        throw new Error(`unexpected fetch for ${url}`);
      });
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);
      await flushGroupsFetch();

      fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));
      const [select] = await screen.findAllByLabelText('Track into');
      fireEvent.click(select);
      fireEvent.click(await screen.findByText('Family'));
      fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

      await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
      expect(screen.queryByText("Couldn't track this train. Try again.")).not.toBeInTheDocument();
    });

    it('does not show the prompt at all when the user has zero groups', async () => {
      const fetchMock = mockFetchByUrl({ groups: groupsResponse([]) });
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

      fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

      await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
      expect(screen.queryAllByLabelText('Track into')).toHaveLength(0);
    });
  });
});
