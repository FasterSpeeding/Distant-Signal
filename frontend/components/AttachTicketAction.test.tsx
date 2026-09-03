import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AttachTicketAction } from './AttachTicketAction';
import type { TrackedTrainListItem } from '@/lib/types';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/track/mine',
  useSearchParams: () => new URLSearchParams(''),
}));

function train(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-08-31',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: '2026-08-31T18:32:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    status: 'en_route',
    delayMinutes: 4,
    trackedAt: '2026-08-31T12:00:00Z',
    ...overrides,
  };
}

describe('AttachTicketAction', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders nothing when there are no tracked trains to attach to', () => {
    const { container } = renderWithMantine(<AttachTicketAction ticketId={5} trains={[]} />);
    expect(container.querySelector('button')).not.toBeInTheDocument();
  });

  it('the Attach button is disabled until a train is selected', () => {
    renderWithMantine(<AttachTicketAction ticketId={5} trains={[train()]} />);
    expect(screen.getByRole('button', { name: 'Attach' })).toBeDisabled();
  });

  it('shows station names in the option label when the backend resolved them', () => {
    renderWithMantine(
      <AttachTicketAction
        ticketId={5}
        trains={[train({ pinOriginName: 'London Waterloo', pinDestinationName: 'Woking' })]}
      />,
    );
    fireEvent.mouseDown(screen.getAllByLabelText('Attach to one of your tracked trains')[0]);
    expect(screen.getByText(/London Waterloo \(WAT\) → Woking \(WOK\)/)).toBeInTheDocument();
  });

  it('on success, POSTs to the attach route and refreshes the page', async () => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(JSON.stringify({ ticketId: 5, trackedTrainId: 1 }), { status: 200 }),
    );
    renderWithMantine(<AttachTicketAction ticketId={5} trains={[train({ id: 1 })]} />);

    fireEvent.mouseDown(screen.getAllByLabelText('Attach to one of your tracked trains')[0]);
    const option = await screen.findByText(/WAT → WOK/);
    fireEvent.click(option);
    fireEvent.click(screen.getByRole('button', { name: 'Attach' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith(
        '/api/Train/tickets/5/attach',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trackingId: 1 }) }),
      );
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('on a 409 (already attached), shows an inline conflict message', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('ticket is already attached to a tracked train', { status: 409 }));
    renderWithMantine(<AttachTicketAction ticketId={5} trains={[train({ id: 1 })]} />);

    fireEvent.mouseDown(screen.getAllByLabelText('Attach to one of your tracked trains')[0]);
    const option = await screen.findByText(/WAT → WOK/);
    fireEvent.click(option);
    fireEvent.click(screen.getByRole('button', { name: 'Attach' }));

    expect(await screen.findByText('This ticket has already been attached to a train.')).toBeInTheDocument();
  });
});
