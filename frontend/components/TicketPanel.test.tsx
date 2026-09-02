import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TicketPanel } from './TicketPanel';
import * as api from '@/lib/api';

vi.mock('@/lib/api');
// TicketPanel's 200-with-tickets and empty-array branches render the real
// TicketEntryForm (Task 6), which calls useRouter() from next/navigation --
// same workaround as PinToggle.test.tsx and TicketEntryForm.test.tsx use for
// the same reason (useRouter() throws outside an app router context).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/train/by-id/1',
  useSearchParams: () => new URLSearchParams(''),
}));

function session(authenticated: boolean) {
  return { authenticated, id: authenticated ? 'user-1' : null, email: null, name: null };
}

describe('TicketPanel', () => {
  beforeEach(() => {
    vi.mocked(api.getDelayRepayEstimate).mockResolvedValue(null);
  });

  it('401 (not logged in): shows a login nudge to attach a ticket', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(false));
    renderWithMantine(await TicketPanel({ trackingId: 1 }));
    expect(screen.getByRole('link', { name: 'Log in to attach a ticket to this journey' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Ftrain%2Fby-id%2F1',
    );
  });

  it('404 (logged in, not the owner): renders nothing', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.mocked(api.getTicketsForTrackedTrain).mockResolvedValue(null);
    const element = await TicketPanel({ trackingId: 1 });
    renderWithMantine(element);
    // Not `toBeEmptyDOMElement()` on the container: MantineProvider injects
    // <style> tags into the render tree regardless, so the container is
    // never literally empty (see EtaBadge.test.tsx and
    // RepresentativeInfo.test.tsx for the same established workaround on
    // other `return null` components). Assert no interactive content
    // rendered instead.
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('200 with an empty array (owner, no ticket yet): shows the add-a-ticket entry point', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.mocked(api.getTicketsForTrackedTrain).mockResolvedValue([]);
    renderWithMantine(await TicketPanel({ trackingId: 1 }));
    expect(screen.getByRole('button', { name: 'Add a ticket for this journey' })).toBeInTheDocument();
  });

  it('200 with tickets: renders each ticket and its own delay-repay estimate, plus an add-another affordance', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.mocked(api.getTicketsForTrackedTrain).mockResolvedValue([
      {
        id: 1,
        trackedTrainId: 1,
        operator: 'LNER',
        ticketType: 'single',
        originCrs: 'KGX',
        destinationCrs: 'EDB',
        originName: null,
        destinationName: null,
        source: 'manual',
        createdAt: '2026-08-29T12:00:00Z',
      },
    ]);
    vi.mocked(api.getDelayRepayEstimate).mockResolvedValue({
      delayMinutes: 45,
      estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: 'x' },
      claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
      disclaimer: 'y',
    });
    renderWithMantine(await TicketPanel({ trackingId: 1 }));
    expect(screen.getByText(/LNER/)).toBeInTheDocument();
    expect(screen.getByText(/KGX → EDB/)).toBeInTheDocument();
    expect(screen.getByText(/50% of your fare/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add another ticket' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
  });

  it('clicking Delete for a ticket DELETEs that exact ticket id', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.mocked(api.getTicketsForTrackedTrain).mockResolvedValue([
      {
        id: 7,
        trackedTrainId: 1,
        operator: 'LNER',
        ticketType: null,
        originCrs: null,
        destinationCrs: null,
        originName: null,
        destinationName: null,
        source: 'manual',
        createdAt: '2026-08-29T12:00:00Z',
      },
    ]);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })));

    renderWithMantine(await TicketPanel({ trackingId: 1 }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/tickets/7', { method: 'DELETE' });
    });
    vi.unstubAllGlobals();
  });

  it('multiple tickets: fetches a delay-repay estimate per ticket, not just the first', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.mocked(api.getTicketsForTrackedTrain).mockResolvedValue([
      { id: 1, trackedTrainId: 1, operator: 'LNER', ticketType: null, originCrs: null, destinationCrs: null, originName: null, destinationName: null, source: 'manual', createdAt: '2026-08-29T12:00:00Z' },
      { id: 2, trackedTrainId: 1, operator: 'CrossCountry', ticketType: null, originCrs: null, destinationCrs: null, originName: null, destinationName: null, source: 'manual', createdAt: '2026-08-29T13:00:00Z' },
    ]);
    await TicketPanel({ trackingId: 1 });
    expect(api.getDelayRepayEstimate).toHaveBeenCalledWith(1, 1);
    expect(api.getDelayRepayEstimate).toHaveBeenCalledWith(1, 2);
  });
});
