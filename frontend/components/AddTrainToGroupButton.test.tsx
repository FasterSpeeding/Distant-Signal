import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AddTrainToGroupButton } from './AddTrainToGroupButton';

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
        serviceDate: '2026-09-11',
        pinOriginCrs: 'WOK',
        pinDestinationCrs: 'WAT',
        pinOriginName: 'Woking',
        pinDestinationName: 'London Waterloo',
        pinScheduledDeparture: '2026-09-11T08:00:00Z',
        resolutionStatus: 'resolved',
        trainUid: 'A12345',
        status: 'en_route',
        delayMinutes: 0,
        trackedAt: '2026-09-10T00:00:00Z',
        customName: null,
      },
      {
        id: 2,
        serviceDate: '2026-09-11',
        pinOriginCrs: 'CLJ',
        pinDestinationCrs: 'VIC',
        pinOriginName: 'Clapham Junction',
        pinDestinationName: 'London Victoria',
        pinScheduledDeparture: '2026-09-11T09:00:00Z',
        resolutionStatus: 'resolved',
        trainUid: 'B67890',
        status: 'en_route',
        delayMinutes: 0,
        trackedAt: '2026-09-10T00:00:00Z',
        customName: null,
      },
    ]),
    { status: 200 },
  );
}

describe('AddTrainToGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('fetches /api/Train/mine on open and excludes already-shared trains', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(mineResponse());

    renderWithMantine(<AddTrainToGroupButton groupId="grp-1" excludeTrainSubscriptionIds={[2]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add one of my trains' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/Train/mine'));
    const [select] = await screen.findAllByLabelText('Tracked train');
    fireEvent.click(select);
    // Note: the dropdown listbox is not scoped with `within(screen.getByRole('dialog'))`
    // here -- Mantine's Combobox dropdown renders in its own portal, as a sibling of the
    // Modal's dialog element in the DOM, not nested inside it (confirmed by running this
    // test and inspecting the rendered DOM; `AttachTicketAction.test.tsx` queries its own
    // Select dropdown the same unscoped way).
    expect(await screen.findByText(/Woking/)).toBeInTheDocument();
    expect(screen.queryByText(/Clapham Junction/)).not.toBeInTheDocument();
  });

  it('POSTs the chosen trainSubscriptionId and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/Train/mine') return Promise.resolve(mineResponse());
      return Promise.resolve(new Response(null, { status: 204 }));
    });

    renderWithMantine(<AddTrainToGroupButton groupId="grp-1" excludeTrainSubscriptionIds={[]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add one of my trains' }));
    const [select] = await screen.findAllByLabelText('Tracked train');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText(/Woking/));
    fireEvent.click(screen.getByRole('button', { name: 'Add to group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/groups/grp-1/trains',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trainSubscriptionId: 1 }) }),
      );
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });
});
