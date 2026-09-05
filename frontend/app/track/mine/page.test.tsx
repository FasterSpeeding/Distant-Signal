import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import MyTrackedTrainsPage from './page';
import * as api from '@/lib/api';
import type { TrackedTrainListItem, TicketListItem } from '@/lib/types';

vi.mock('@/lib/api');
// The not-logged-in prompt is AutoOpenLoginPrompt -> LoginPromptModal,
// which calls useLoginHref() (usePathname()/useSearchParams() under the
// hood) -- same stub AuthStatus.test.tsx and TicketPanel.test.tsx use for
// the same reason. This page also renders AttachTicketAction and
// DeleteTicketButton, both of which call useRouter() from next/navigation
// -- same workaround TicketPanel.test.tsx/TicketEntryForm.test.tsx use for
// the same reason (useRouter() throws outside an app router context).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/track/mine',
  useSearchParams: () => new URLSearchParams(''),
}));

function train(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-08-31',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    // null (bare-code rendering) by default -- see app/page.test.tsx's
    // `item()` for the same rationale. The name-rendering path gets its
    // own dedicated test below.
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: '2026-08-31T18:32:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    status: 'en_route',
    delayMinutes: 4,
    trackedAt: '2026-08-31T12:00:00Z',
    ...overrides,
    customName: overrides.customName ?? null,
  };
}

function ticket(overrides: Partial<TicketListItem> = {}): TicketListItem {
  return {
    id: 1,
    trackedTrainId: 1,
    operator: 'LNER',
    ticketType: 'Off-Peak Day Single',
    originCrs: 'KGX',
    destinationCrs: 'EDB',
    originName: null,
    destinationName: null,
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
    customName: overrides.customName ?? null,
  };
}

describe('MyTrackedTrainsPage (merged trains + tickets)', () => {
  it('null (not logged in): shows an auto-opened login prompt modal', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    vi.mocked(api.getMyTickets).mockResolvedValue(null);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('Log in required')).toBeInTheDocument();
    expect(screen.getByText("Log in to see the trains and tickets you're tracking.")).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Ftrack%2Fmine',
    );
  });

  it('no trains and no tickets: shows the empty state with a working link to /track', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/haven't tracked any trains or added any tickets yet/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a train' })).toHaveAttribute('href', '/track');
  });

  it('a train with no tickets: renders just the train row, no ticket content under it', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
    expect(screen.queryByText('LNER')).not.toBeInTheDocument();
    expect(screen.queryByText('Tickets not yet attached to a train')).not.toBeInTheDocument();
  });

  it('a train with an attached ticket: renders the ticket summary and its Delay Repay estimate under the train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ trackedTrainId: 1 })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
    expect(screen.getByText(/LNER/)).toBeInTheDocument();
    expect(screen.getByText(/KGX → EDB/)).toBeInTheDocument();
    expect(screen.getByText(/50% of your fare/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /See how to claim from the operator/ })).toHaveAttribute(
      'href',
      'https://delayrepay.lner.co.uk/delayrepayV2/',
    );
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
  });

  it('clicking Delete on an attached ticket row DELETEs that exact ticket id', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 9, trackedTrainId: 1 })]);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })));

    renderWithMantine(await MyTrackedTrainsPage());
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/tickets/9', { method: 'DELETE' });
    });
    vi.unstubAllGlobals();
  });

  it('multiple tickets on one train: renders every one of them, not just the first', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([
      ticket({ id: 1, trackedTrainId: 1, operator: 'LNER' }),
      ticket({ id: 2, trackedTrainId: 1, operator: 'CrossCountry', claimUrl: 'https://delayrepay.crosscountrytrains.co.uk/' }),
    ]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getAllByRole('link', { name: /See how to claim from the operator/ })).toHaveLength(2);
  });

  it('a ticket attached to a DIFFERENT tracked train does not render under this one', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 1 })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 1, trackedTrainId: 999 })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
    expect(screen.queryByText('LNER')).not.toBeInTheDocument();
  });

  // Part A/B: a standalone ticket (trackedTrainId: null) not yet attached
  // to anything.
  it('a standalone (unattached) ticket: renders in its own section with an attach action and a track-a-new-train link', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 1, pinOriginCrs: 'WAT', pinDestinationCrs: 'WOK' })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([
      ticket({
        id: 5,
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
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('Tickets not yet attached to a train')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a new train for this ticket' })).toHaveAttribute(
      'href',
      '/track?origin=KGX&ticketId=5',
    );
    // The attach action offers the caller's own already-tracked train.
    // Mantine's Select associates its label with more than one element
    // (the visible input plus its combobox option list), so
    // getAllByLabelText (not getByLabelText) is the correct query here.
    expect(screen.getAllByLabelText('Attach to one of your tracked trains').length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
  });

  it('an unattached ticket with no origin: the track-a-new-train link omits the origin param', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 6, trackedTrainId: null, originCrs: null })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: 'Track a new train for this ticket' })).toHaveAttribute(
      'href',
      '/track?ticketId=6',
    );
  });

  it('no tracked trains yet: the attach-to-existing-train action is not offered (nothing to attach to)', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 5, trackedTrainId: null })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.queryByLabelText('Attach to one of your tracked trains')).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a new train for this ticket' })).toBeInTheDocument();
  });

  it('resolved train with a trainUid: links to the canonical /train/{uid}/{date} URL', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/C21373/2026-08-31');
  });

  it('pending train: links to the by-id detail route', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      train({ resolutionStatus: 'pending', trainUid: null, status: null, delayMinutes: null }),
    ]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/by-id/1');
  });

  it('renders a delay badge for a resolved, delayed train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ delayMinutes: 12 })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('12m late')).toBeInTheDocument();
  });

  it('renders station names when the backend resolved them, not just bare codes', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      train({ pinOriginName: 'London Waterloo', pinDestinationName: 'Woking' }),
    ]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/London Waterloo \(WAT\) → Woking \(WOK\)/)).toBeInTheDocument();
  });

  it('falls back to the bare code, not "null" or an empty label, when a name did not resolve', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ pinOriginName: null, pinDestinationName: null })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
    expect(screen.queryByText(/null/i)).not.toBeInTheDocument();
  });

  it('renders train rows in the same order getMyTrackedTrains returned them', async () => {
    const first = train({ id: 1, pinOriginCrs: 'WAT' });
    const second = train({ id: 2, pinOriginCrs: 'PAD' });
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([first, second]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());

    const links = screen.getAllByRole('link');
    const originOrder = links
      .map((link) => link.textContent ?? '')
      .filter((text) => text.startsWith('WAT') || text.startsWith('PAD'));
    expect(originOrder).toEqual([expect.stringMatching(/^WAT/), expect.stringMatching(/^PAD/)]);
  });

  it('renders "Track a new train" and "Add a ticket" entry-point links beside the title', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: 'Track a new train' })).toHaveAttribute('href', '/track');
    expect(screen.getByRole('link', { name: 'Add a ticket' })).toHaveAttribute(
      'href',
      '/track/mine/add-ticket',
    );
  });
});
