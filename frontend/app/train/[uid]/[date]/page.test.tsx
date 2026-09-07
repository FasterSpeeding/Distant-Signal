import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
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
});
