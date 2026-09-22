import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import JourneyDetailPage from './page';
import * as api from '@/lib/api';
import type { JourneyDetail, JourneyLegDetail } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getJourney: vi.fn() };
});

vi.mock('next/navigation', () => ({
  notFound: () => {
    throw new Error('NEXT_NOT_FOUND');
  },
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/journeys/1',
  useSearchParams: () => new URLSearchParams(''),
}));

function baseLeg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'KGX',
    originName: null,
    destinationCrs: 'EDB',
    destinationName: null,
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'unmatched',
    trackedTrainState: null,
    legSkip: null,
    ...overrides,
  };
}

function baseJourney(overrides: Partial<JourneyDetail> = {}): JourneyDetail {
  return {
    id: 1,
    customName: null,
    createdAt: '2026-09-22T10:00:00Z',
    legs: [baseLeg()],
    isOwner: true,
    ...overrides,
  };
}

describe('JourneyDetailPage title (M18)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('defaults the <h1> to the route/date when there is no custom name, using resolved names when available', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    vi.mocked(api.getJourney).mockResolvedValue(
      baseJourney({ legs: [baseLeg({ originName: 'London Kings Cross', destinationName: 'Edinburgh' })] }),
    );

    renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id: '1' }) }));
    await screen.findAllByText(/No scheduled trains match this window\./);

    expect(
      screen.getByRole('heading', { level: 1, name: 'London Kings Cross (KGX) → Edinburgh (EDB), 22 Sept 2026' }),
    ).toBeInTheDocument();
    expect(screen.queryByText('Tracked journey')).not.toBeInTheDocument();
  });

  it('falls back to bare CRS codes in the title when no station name resolved', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    vi.mocked(api.getJourney).mockResolvedValue(baseJourney());

    renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id: '1' }) }));
    await screen.findAllByText(/No scheduled trains match this window\./);

    expect(screen.getByRole('heading', { level: 1, name: 'KGX → EDB, 22 Sept 2026' })).toBeInTheDocument();
  });

  it('still prefers a real customName over the computed default', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    vi.mocked(api.getJourney).mockResolvedValue(baseJourney({ customName: 'My commute' }));

    renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id: '1' }) }));
    await screen.findAllByText(/No scheduled trains match this window\./);

    expect(screen.getByRole('heading', { level: 1, name: 'My commute' })).toBeInTheDocument();
  });

  it('spans a multi-leg journey from the first leg\'s origin to the last leg\'s destination', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    vi.mocked(api.getJourney).mockResolvedValue(
      baseJourney({
        legs: [
          baseLeg({ id: 1, originCrs: 'KGX', destinationCrs: 'YRK', matchMode: 'manual' }),
          baseLeg({ id: 2, originCrs: 'YRK', destinationCrs: 'NCL' }),
        ],
      }),
    );

    renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id: '1' }) }));
    await screen.findAllByText(/No scheduled trains match this window\./);

    expect(screen.getByRole('heading', { level: 1, name: 'KGX → NCL, 22 Sept 2026' })).toBeInTheDocument();
  });
});
