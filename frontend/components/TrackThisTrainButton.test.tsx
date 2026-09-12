import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { ReactElement } from 'react';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { GroupSummariesProvider } from '@/lib/useGroupSummaries';
import { TrackThisTrainButton } from './TrackThisTrainButton';
import type { GroupSummary } from '@/lib/types';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

/** `useGroupSummaries` now reads from `GroupSummariesProvider`'s context
 * instead of fetching `/api/groups` itself (see `lib/useGroupSummaries.tsx`'s
 * own doc comment) -- so unlike before, no `flushGroupsFetch`-style wait is
 * needed: the value supplied here is present from this component's very
 * first render. `groups` defaults to `[]`, the zero-groups case every
 * pre-existing test in this file exercises; only the "group-share
 * destination prompt" tests below override it. */
function renderWithGroups(ui: ReactElement, groups: GroupSummary[] | null = []) {
  return renderWithMantine(<GroupSummariesProvider groups={groups}>{ui}</GroupSummariesProvider>);
}

/** Routes a mocked `fetch` by URL, the same shape
 * `TrackTrainForm.test.tsx`'s own `mockFetchByUrl` helper uses: the
 * by-uid track call and the ticket-attach follow-up are configured
 * independently so a test can make either one fail/succeed without the
 * other. */
function mockFetchByUrl(
  options: {
    track?: () => Response;
    attach?: () => Response | Promise<Response>;
  } = {},
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
    renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

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
    renderWithGroups(<TrackThisTrainButton uid="C11052/../mine" date="2026-09-07" />);

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
    renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('makes no ticket-attach call when attachTicketId is absent', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

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
    renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

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
    renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('still navigates when the ticket-attach follow-up returns a 409', async () => {
    const fetchMock = mockFetchByUrl({
      attach: () => new Response('ticket is already attached to a tracked train', { status: 409 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('opens the login prompt and does not navigate on a 401', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ track: () => new Response('unauthorized', { status: 401 }) }));
    renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    expect(await screen.findByText('Log in to track this train.')).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('shows an error and does not navigate on a 500', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ track: () => new Response('boom', { status: 500 }) }));
    renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

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
        if (/\/track$/.test(url)) return pending;
        throw new Error(`unexpected fetch for ${url}`);
      }),
    );
    renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    const button = screen.getByRole('button', { name: 'Track this train' });
    fireEvent.click(button);

    await waitFor(() => expect(screen.getByRole('button', { name: 'Tracking…' })).toBeDisabled());

    resolveTrack(new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }));
    await waitFor(() => expect(pushMock).toHaveBeenCalled());
  });

  // Shared-groups follow-up: the "Personal or one of your groups?" prompt.
  describe('group-share destination prompt', () => {
    const GROUPS_FIXTURE: GroupSummary[] = [
      { id: 'grp-1', name: 'Family', role: 'owner', memberCount: 3 },
      { id: 'grp-2', name: 'Commuters', role: 'member', memberCount: 5 },
    ];

    it('opens the destination prompt instead of tracking immediately when the user has at least one group', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />, GROUPS_FIXTURE);

      fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

      expect((await screen.findAllByLabelText('Track into')).length).toBeGreaterThan(0);
      const trackCalls = fetchMock.mock.calls.filter((args: unknown[]) => /\/track$/.test(String(args[0])));
      expect(trackCalls).toHaveLength(0);
      expect(pushMock).not.toHaveBeenCalled();
    });

    it('confirming with the default "Personal" selection tracks privately, with no group-share call', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />, GROUPS_FIXTURE);

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
      const fetchMock = vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        if (/\/api\/Train\/by-uid\/.+\/track$/.test(url)) {
          return Promise.resolve(new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }));
        }
        if (/\/api\/groups\/grp-1\/trains$/.test(url)) return Promise.resolve(new Response(null, { status: 204 }));
        throw new Error(`unexpected fetch for ${url}`);
      });
      vi.stubGlobal('fetch', fetchMock);
      renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />, GROUPS_FIXTURE);

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
        if (/\/track$/.test(url)) {
          return Promise.resolve(new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }));
        }
        if (/\/groups\/grp-1\/trains$/.test(url)) return Promise.reject(new Error('network blip'));
        throw new Error(`unexpected fetch for ${url}`);
      });
      vi.stubGlobal('fetch', fetchMock);
      renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />, GROUPS_FIXTURE);

      fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));
      const [select] = await screen.findAllByLabelText('Track into');
      fireEvent.click(select);
      fireEvent.click(await screen.findByText('Family'));
      fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

      await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
      expect(screen.queryByText("Couldn't track this train. Try again.")).not.toBeInTheDocument();
    });

    it('does not show the prompt at all when the user has zero groups', async () => {
      const fetchMock = mockFetchByUrl();
      vi.stubGlobal('fetch', fetchMock);
      renderWithGroups(<TrackThisTrainButton uid="C11052" date="2026-09-07" />, []);

      fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

      await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
      expect(screen.queryAllByLabelText('Track into')).toHaveLength(0);
    });
  });
});
