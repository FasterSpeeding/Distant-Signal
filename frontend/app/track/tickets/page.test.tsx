import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import MyTicketsPage from './page';
import * as api from '@/lib/api';
import type { TicketListItem } from '@/lib/types';

vi.mock('@/lib/api');
// This page now renders TicketEntryForm (Part A's "Add a ticket" entry
// point), which calls useRouter() from next/navigation -- same workaround
// TicketPanel.test.tsx/TicketEntryForm.test.tsx use for the same reason
// (useRouter() throws outside an app router context).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/track/tickets',
  useSearchParams: () => new URLSearchParams(''),
}));

function item(overrides: Partial<TicketListItem> = {}): TicketListItem {
  return {
    id: 1,
    trackedTrainId: 1,
    operator: 'LNER',
    ticketType: 'Off-Peak Day Single',
    originCrs: 'KGX',
    destinationCrs: 'EDB',
    source: 'manual',
    createdAt: '2026-08-31T12:00:00Z',
    serviceDate: '2026-08-31',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'EDB',
    pinScheduledDeparture: '2026-08-31T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'A12345',
    status: 'en_route',
    delayMinutes: 45,
    estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: 'x' },
    claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
    disclaimer: 'This is a rough, community-sourced estimate...',
    ...overrides,
  };
}

describe('MyTicketsPage', () => {
  it('null (not logged in): shows a login nudge', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue(null);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByRole('link', { name: 'Log in to see your tickets' })).toHaveAttribute(
      'href',
      '/api/auth/login',
    );
  });

  it('empty array: shows the empty state with a working link to /track', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByText(/haven't added any tickets yet/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a train' })).toHaveAttribute('href', '/track');
  });

  it('resolved ticket with a trainUid: links to the canonical /train/{uid}/{date} URL', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([item()]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByRole('link', { name: /31 Aug 2026/ })).toHaveAttribute('href', '/train/A12345/2026-08-31');
  });

  it('pending ticket: links to the by-id detail route', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([
      item({ resolutionStatus: 'pending', trainUid: null, status: null, delayMinutes: null, estimate: null }),
    ]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByRole('link', { name: /31 Aug 2026/ })).toHaveAttribute('href', '/train/by-id/1');
  });

  it('every row renders its ticket summary and a DelayRepayEstimate block with the verbatim disclaimer', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([item()]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByText(/LNER/)).toBeInTheDocument();
    expect(screen.getByText(/KGX → EDB/)).toBeInTheDocument();
    expect(screen.getByText(/50% of your fare/)).toBeInTheDocument();
    expect(screen.getByText('This is a rough, community-sourced estimate...')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /See how to claim from the operator/ })).toHaveAttribute(
      'href',
      'https://delayrepay.lner.co.uk/delayrepayV2/',
    );
  });

  it('multiple tickets: renders one DelayRepayEstimate block per row, not just the first', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([
      item({ id: 1, operator: 'LNER' }),
      item({ id: 2, operator: 'CrossCountry', claimUrl: 'https://delayrepay.crosscountrytrains.co.uk/' }),
    ]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getAllByRole('link', { name: /See how to claim from the operator/ })).toHaveLength(2);
  });

  // Part A: a standalone ticket (no tracked train attached yet) --
  // trackedTrainId and every train-context field come back null.
  it('a standalone ticket (no tracked train yet) renders with no link and no crash', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([
      item({
        trackedTrainId: null,
        serviceDate: null,
        pinOriginCrs: null,
        pinDestinationCrs: null,
        pinScheduledDeparture: null,
        resolutionStatus: null,
        trainUid: null,
        status: null,
        delayMinutes: null,
        estimate: null,
      }),
    ]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByText('Not yet attached to a tracked train')).toBeInTheDocument();
    // Still gets its Delay Repay block (claimUrl/disclaimer stay
    // unconditionally populated even with no train data at all).
    expect(screen.getByRole('link', { name: /See how to claim from the operator/ })).toBeInTheDocument();
  });

  it('renders the "Add a ticket" entry point (Part A\'s standalone-upload entry point)', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByRole('button', { name: 'Add a ticket' })).toBeInTheDocument();
  });
});
