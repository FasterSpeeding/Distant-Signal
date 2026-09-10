import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrackedTrainByUidPage, { toJourneyState } from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { PublicTrainState, TrackedTrainListItem } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getPublicTrainByUidAndDate: vi.fn(),
    getMyTrackedTrains: vi.fn(),
    getTrackedTrainById: vi.fn(),
  };
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
// `TicketPanel` is itself an async Server Component (its own `getSession()`/
// ownership-probe fetches) -- React's plain DOM test renderer, used via
// `renderWithMantine` here, can't render an async function component at all
// (an RSC-only capability) -- same workaround
// app/train/by-id/[trackingId]/page.test.tsx already uses.
vi.mock('@/components/TicketPanel', () => ({
  TicketPanel: () => null,
}));

// Logged-out by default -- individual tests in the "tracking overlay"
// describe block below override this per case.
beforeEach(() => {
  vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
});

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
    journeyStops: null,
    mayHaveArrived: false,
    ...overrides,
  };
}

async function renderPage(uid = 'W12345', date = '2026-08-31') {
  const element = await TrackedTrainByUidPage({ params: Promise.resolve({ uid, date }) });
  return renderWithMantine(element);
}

/** `GET /Train/mine`'s per-item shape -- see `TrackedTrainListItem`'s own
 * doc comment in `lib/types.ts`. `trainUid`/`serviceDate` default to a
 * match against `renderPage()`'s own defaults, since the overlay tests
 * below are all about whether those two fields line up with the URL. */
function trackedTrainListItem(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 7,
    serviceDate: '2026-08-31',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: null,
    resolutionStatus: 'resolved',
    trainUid: 'W12345',
    status: 'en_route',
    delayMinutes: 0,
    trackedAt: '2026-08-31T10:00:00Z',
    customName: null,
    ...overrides,
  };
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

describe('TrackedTrainByUidPage tracking overlay', () => {
  beforeEach(() => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
  });

  it('renders the plain public view when logged out', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    await renderPage();
    expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Rename/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
    expect(api.getTrackedTrainById).not.toHaveBeenCalled();
  });

  it('renders the plain public view when logged in but not tracking this exact train', async () => {
    // Neither field matches the URL's own uid/date -- a real tracked train
    // of this visitor's, just not this one.
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      trackedTrainListItem({ trainUid: 'OTHER', serviceDate: '2026-08-31' }),
      trackedTrainListItem({ trainUid: 'W12345', serviceDate: '2020-01-01' }),
    ]);
    await renderPage();
    expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Rename/i })).not.toBeInTheDocument();
    expect(api.getTrackedTrainById).not.toHaveBeenCalled();
  });

  it('renders owner controls instead of Track this train when the visitor already tracks this exact train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      trackedTrainListItem({ id: 7, trainUid: 'W12345', serviceDate: '2026-08-31' }),
    ]);
    await renderPage();
    expect(screen.queryByRole('button', { name: 'Track this train' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Rename/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
    // ShareButton stays regardless of ownership.
    expect(screen.getByRole('button', { name: /share/i })).toBeInTheDocument();
    // Finding 4: `TrackedTrainListItem` (the `GET /Train/mine` match)
    // already carries everything the owner controls/journey overlay need
    // -- no second `GET /Train/{id}` fetch should ever fire.
    expect(api.getTrackedTrainById).not.toHaveBeenCalled();
  });

  // Finding 1: a renamed/pinned train's custom name must show on the
  // overlay -- `toJourneyState(train)` alone hardcodes `customName: null`
  // (there's nothing per-subscriber on the public response to read it
  // from), so the page must overlay the visitor's own `customName` from
  // the `GET /Train/mine` match once one is found.
  it('shows the tracking owner custom name on the overlay', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      trackedTrainListItem({ id: 7, trainUid: 'W12345', serviceDate: '2026-08-31', customName: 'My commute' }),
    ]);
    await renderPage();
    expect(screen.getByText('My commute')).toBeInTheDocument();
  });

  // Finding 2: `getMyTrackedTrains()` is an auxiliary "am I tracking this?"
  // check, not the primary content of this public page -- a transient
  // failure of it (matching app/page.tsx's own `.catch(() => null)`
  // precedent) must degrade to the plain public view for every visitor,
  // not crash the whole page.
  it('falls back to the plain public view when getMyTrackedTrains() itself fails', async () => {
    vi.mocked(api.getMyTrackedTrains).mockRejectedValue(new Error('boom'));
    await renderPage();
    expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Rename/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
  });
});

describe('toJourneyState', () => {
  it('carries journeyStops through unchanged from PublicTrainState', () => {
    const stop = {
      crs: 'RDG',
      name: 'Reading',
      tiploc: null,
      kind: 'Origin' as const,
      scheduledArrival: null,
      scheduledDeparture: '2026-09-08T08:00:00Z',
      actualArrival: null,
      actualDeparture: null,
      estimatedArrival: null,
      estimatedDeparture: null,
      lastEventType: null,
      variationStatus: null,
      delayMinutes: null,
    };
    const result = toJourneyState({
      trainsId: 1,
      trainUid: 'X12345',
      serviceDate: '2026-09-08',
      originCrs: 'RDG',
      originName: 'Reading',
      destinationCrs: 'WAT',
      destinationName: 'London Waterloo',
      scheduledDeparture: '2026-09-08T08:00:00Z',
      callingPoints: null,
      trainId: null,
      status: null,
      lastReportedLocation: null,
      lastEventType: null,
      delayMinutes: null,
      nextCallingPoint: null,
      etaNext: null,
      etaSource: null,
      journeyStops: [stop],
      mayHaveArrived: false,
    });

    expect(result.journeyStops).toEqual([stop]);
  });

  it('carries mayHaveArrived through from PublicTrainState', () => {
    const result = toJourneyState(
      publicTrainState({
        mayHaveArrived: true,
      }),
    );

    expect(result.mayHaveArrived).toBe(true);
  });
});
