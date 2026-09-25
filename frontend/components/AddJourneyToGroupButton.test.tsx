import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AddJourneyToGroupButton } from './AddJourneyToGroupButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

function mineResponse() {
  return new Response(
    JSON.stringify([
      {
        id: 1,
        customName: null,
        createdAt: '2026-09-10T00:00:00Z',
        legId: 10,
        originCrs: 'WOK',
        destinationCrs: 'WAT',
        matchMode: 'auto',
        trainSubscriptionId: 1,
        resolutionStatus: 'resolved',
        status: 'en_route',
        delayMinutes: 0,
      },
      {
        id: 2,
        customName: null,
        createdAt: '2026-09-10T00:00:00Z',
        legId: 20,
        originCrs: 'CLJ',
        destinationCrs: 'VIC',
        matchMode: 'auto',
        trainSubscriptionId: 2,
        resolutionStatus: 'resolved',
        status: 'en_route',
        delayMinutes: 0,
      },
    ]),
    { status: 200 },
  );
}

describe('AddJourneyToGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('fetches /api/Journeys/mine on open and excludes already-shared journeys', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(mineResponse());

    renderWithMantine(<AddJourneyToGroupButton groupId="grp-1" excludeJourneyIds={[2]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add one of my journeys' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/mine'));
    const [select] = await screen.findAllByLabelText('Journey');
    fireEvent.click(select);
    expect(await screen.findByText(/WOK → WAT/)).toBeInTheDocument();
    expect(screen.queryByText(/CLJ → VIC/)).not.toBeInTheDocument();
  });

  it('POSTs the chosen journeyId and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/Journeys/mine') return Promise.resolve(mineResponse());
      return Promise.resolve(new Response(null, { status: 204 }));
    });

    renderWithMantine(<AddJourneyToGroupButton groupId="grp-1" excludeJourneyIds={[]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add one of my journeys' }));
    const [select] = await screen.findAllByLabelText('Journey');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText(/WOK → WAT/));
    fireEvent.click(screen.getByRole('button', { name: 'Add to group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/groups/grp-1/journeys',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ journeyId: 1 }) }),
      );
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  // Bug: `router.refresh()` preserves client component state, so a
  // `submitting` flag left `true` when it fired stayed `true` forever --
  // `handleOpen` resets `selected`/`error` but never touched `submitting`,
  // permanently disabling/spinning "Add to group" on every future open. See
  // `AddTrainToGroupButton.test.tsx`'s identical regression test for the
  // full rationale.
  it('"Add to group" is usable again on a later open, not stuck disabled from the previous add', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/Journeys/mine') return Promise.resolve(mineResponse());
      return Promise.resolve(new Response(null, { status: 204 }));
    });

    renderWithMantine(<AddJourneyToGroupButton groupId="grp-1" excludeJourneyIds={[]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add one of my journeys' }));
    let [select] = await screen.findAllByLabelText('Journey');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText(/WOK → WAT/));
    fireEvent.click(screen.getByRole('button', { name: 'Add to group' }));
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: 'Add one of my journeys' }));
    [select] = await screen.findAllByLabelText('Journey');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText(/WOK → WAT/));
    expect(screen.getByRole('button', { name: 'Add to group' })).not.toBeDisabled();
  });
});
