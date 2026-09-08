import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrackedTrainByUidPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { PublicTrainState } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getPublicTrainByUidAndDate: vi.fn() };
});
// Mocked so a real Next.js `notFound()` throw (its actual behaviour) doesn't
// require an app-router tree to render -- same pattern as
// app/lines/[id]/page.test.tsx.
const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/train/W12345/2026-08-31',
  useSearchParams: () => new URLSearchParams(''),
}));

/** The PUBLIC response shape (`crates/api/src/data/trains.rs`'s
 * `PublicTrainState`) -- note `trainsId`, a `trains.id`, deliberately NOT
 * named `id` and deliberately NOT a tracking id. */
function publicTrainState(overrides: Partial<PublicTrainState> = {}): PublicTrainState {
  return {
    trainsId: 42,
    trainUid: 'W12345',
    serviceDate: '2026-08-31',
    originCrs: 'WAT',
    originName: null,
    destinationCrs: 'WOK',
    destinationName: null,
    scheduledDeparture: '2026-08-31T18:32:00Z',
    callingPoints: null,
    trainId: '1A23',
    status: 'en_route',
    lastReportedLocation: 'Woking',
    lastEventType: 'DEPARTURE',
    delayMinutes: 0,
    nextCallingPoint: 'Basingstoke',
    etaNext: null,
    etaSource: null,
    ...overrides,
  };
}

async function renderPage(uid = 'W12345', date = '2026-08-31') {
  const element = await TrackedTrainByUidPage({ params: Promise.resolve({ uid, date }) });
  return renderWithMantine(element);
}

describe('TrackedTrainByUidPage error handling', () => {
  beforeEach(() => {
    notFoundMock.mockClear();
  });

  it('still calls notFound() on ApiNotFoundError', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockRejectedValue(new ApiNotFoundError('not found'));
    await expect(renderPage()).rejects.toThrow('NEXT_NOT_FOUND');
    expect(notFoundMock).toHaveBeenCalled();
  });

  it('still propagates a bare Error uncaught', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockRejectedValue(new Error('boom'));
    await expect(renderPage()).rejects.toThrow('boom');
    expect(notFoundMock).not.toHaveBeenCalled();
  });
});

describe('TrackedTrainByUidPage success path', () => {
  // Review finding C3: this page used to feed the public response's own
  // `id` (a `trains.id`) into RenameTrainButton/DeleteTrainButton/
  // TicketPanel as a `trackingId`, which every `/Train/{trackingId}` route
  // reads as a `train_subscriptions.id` -- a different BIGSERIAL space
  // that also starts at 1. Those controls now don't render at all: this
  // page describes a shared, public train and carries no subscription.
  it('renders no owner actions -- there is no subscription id in a public response', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    await renderPage();
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /rename/i })).not.toBeInTheDocument();
  });

  it('renders the shared train journey from the public response', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    await renderPage();
    expect(screen.getByRole('heading', { name: 'Train W12345' })).toBeInTheDocument();
    expect(screen.getByText(/Last reported: Woking/)).toBeInTheDocument();
    expect(screen.getByText(/Next calling point: Basingstoke/)).toBeInTheDocument();
  });

  // The shared `trains` row has no `resolution_status` column -- that is
  // per-subscription state -- so the page derives one. A train with no
  // TRUST `train_id` and no schedule match yet is "pending".
  it('derives a pending state for a bare train_uid with no schedule or TRUST data', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(
      publicTrainState({
        originCrs: null,
        destinationCrs: null,
        scheduledDeparture: null,
        trainId: null,
        status: null,
        lastReportedLocation: null,
        lastEventType: null,
        delayMinutes: null,
        nextCallingPoint: null,
      }),
    );
    await renderPage();
    expect(screen.getByText('Waiting to hear from Network Rail')).toBeInTheDocument();
  });

  it('derives a schedule_matched state when there is schedule data but no TRUST train_id', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(
      publicTrainState({
        destinationName: 'Woking',
        trainId: null,
        status: null,
        lastReportedLocation: null,
        lastEventType: null,
        delayMinutes: null,
        nextCallingPoint: null,
      }),
    );
    await renderPage();
    expect(
      screen.getByText('Matched to a scheduled service — Train W12345 to Woking'),
    ).toBeInTheDocument();
  });

  it('renders a Track this train button for every visitor', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );
    expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
  });

  it('tracks by the uid and date from the URL, not from the response body', async () => {
    // Discriminating: the fixture's own trainUid deliberately differs from
    // the URL segment, so a component wired to the wrong source fails here.
    const fetchMock = vi.fn(
      async () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
    );
    vi.stubGlobal('fetch', fetchMock);
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(
      publicTrainState({ trainUid: 'DIFFERENT' }),
    );
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/by-uid/W12345/2026-08-31/track',
        expect.objectContaining({ method: 'POST' }),
      ),
    );
  });

  // The spec's own §5 exclusion, asserted rather than assumed: this page's
  // CTA must never make a ticket-attach call, because this page has no
  // ticketId convention to source one from.
  it('makes no ticket-attach call after tracking', async () => {
    const fetchMock = vi.fn(
      async () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
    );
    vi.stubGlobal('fetch', fetchMock);
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const attachCalls = fetchMock.mock.calls.filter((args: unknown[]) =>
      String(args[0]).includes('/attach'),
    );
    expect(attachCalls).toHaveLength(0);
  });

  it('still renders no owner actions alongside the new CTA', async () => {
    // Regression guard on this page's whole reason for being read-only:
    // Rename/Delete/tickets all key on a train_subscriptions.id this
    // response does not carry (see the page's own doc comment). Adding a
    // track CTA must not have opened that door.
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );
    expect(screen.queryByRole('button', { name: /Rename/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Delete/i })).not.toBeInTheDocument();
  });

  it('points at the new /trains page for finding other trains', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );
    expect(screen.getByRole('link', { name: 'Find a train' })).toHaveAttribute('href', '/trains');
  });
});
