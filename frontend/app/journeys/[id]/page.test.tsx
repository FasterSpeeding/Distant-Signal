import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import JourneyDetailPage from './page';
import * as api from '@/lib/api';
import type { JourneyDetail, JourneyLegDetail, TrackedTrainState } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getJourney: vi.fn(),
  };
});

vi.mock('next/navigation', () => ({
  notFound: () => {
    throw new Error('NEXT_NOT_FOUND');
  },
  useRouter: () => ({ refresh: vi.fn(), push: vi.fn() }),
  usePathname: () => '/journeys/167',
  useSearchParams: () => new URLSearchParams(''),
}));

// `getSiteOrigin()` (lib/siteOrigin.ts), called unconditionally by this
// page to build `ShareJourneyLinkButton`'s `origin` prop, reads
// `next/headers` when `NEXT_PUBLIC_SITE_URL` isn't set -- there is no Next
// request context in a unit test. Same stub shape
// `app/groups/[id]/page.test.tsx` uses for the same reason.
vi.mock('next/headers', () => ({
  headers: async () => ({ get: () => null }),
}));

function trackedState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-09-22',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'EDB',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'P9E010',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: 22,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: 'EDB',
    scheduleDestinationName: 'Edinburgh',
    scheduleCallingPoints: null,
    journeyStops: [
      {
        crs: 'KGX',
        name: 'London Kings Cross',
        tiploc: null,
        kind: 'Origin',
        scheduledArrival: null,
        scheduledDeparture: '2026-09-22T15:00:00Z',
        actualArrival: null,
        actualDeparture: '2026-09-22T15:00:00Z',
        estimatedArrival: null,
        estimatedDeparture: null,
        lastEventType: 'DEPARTURE',
        variationStatus: null,
        delayMinutes: 0,
        stopStatus: 'Called',
        skipSource: null,
        platform: null,
        plannedPlatform: null,
        platformChanged: false,
      },
      {
        crs: 'YRK',
        name: 'York',
        tiploc: null,
        kind: 'Intermediate',
        scheduledArrival: '2026-09-22T17:00:00Z',
        scheduledDeparture: null,
        actualArrival: null,
        actualDeparture: null,
        estimatedArrival: '2026-09-22T17:22:00Z',
        estimatedDeparture: null,
        lastEventType: null,
        variationStatus: null,
        delayMinutes: null,
        stopStatus: 'Scheduled',
        skipSource: null,
        platform: null,
        plannedPlatform: null,
        platformChanged: false,
      },
    ],
    mayHaveArrived: false,
    customName: null,
    sharedGroupCount: 0,
    ...overrides,
  };
}

function matchedLeg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'KGX',
    originName: null,
    destinationCrs: 'YRK',
    destinationName: null,
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'manual',
    trackedTrainState: trackedState(),
    legSkip: null,
    ...overrides,
  };
}

function baseJourney(overrides: Partial<JourneyDetail> = {}): JourneyDetail {
  return {
    id: 167,
    customName: null,
    createdAt: '2026-09-22T00:00:00Z',
    legs: [matchedLeg()],
    isOwner: false,
    shareLink: null,
    ...overrides,
  };
}

async function renderPage(id = '167') {
  return renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id }) }));
}

describe('JourneyDetailPage', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  // 2026-09-22 UX review finding M18: a null customName no longer falls
  // back to the flat "Tracked journey" -- it defaults to the route.
  it('defaults the <h1> to the route when there is no custom name', async () => {
    vi.mocked(api.getJourney).mockResolvedValue(baseJourney());
    await renderPage();
    expect(screen.getByRole('heading', { level: 1, name: /London Kings Cross \(KGX\) → York \(YRK\)/ })).toBeInTheDocument();
    expect(screen.queryByText('Tracked journey')).not.toBeInTheDocument();
  });

  it('still honours a real custom name when one is set', async () => {
    vi.mocked(api.getJourney).mockResolvedValue(baseJourney({ customName: 'Trip to Grandma\'s' }));
    await renderPage();
    expect(screen.getByRole('heading', { level: 1, name: "Trip to Grandma's" })).toBeInTheDocument();
  });

  // 2026-09-22 UX review finding I24/2.12: no freshness timestamp
  // anywhere on the page.
  it('shows a "Data as of" freshness line', async () => {
    vi.mocked(api.getJourney).mockResolvedValue(baseJourney());
    await renderPage();
    expect(screen.getByText(/Data as of/)).toBeInTheDocument();
  });

  // A single-leg journey has nothing to number or connect.
  it('does not number legs or show a per-leg summary line for a single-leg journey', async () => {
    vi.mocked(api.getJourney).mockResolvedValue(baseJourney());
    await renderPage();
    expect(screen.queryByText(/Leg 1 of/)).not.toBeInTheDocument();
    expect(screen.queryByText(/^Leg 1 /)).not.toBeInTheDocument();
  });

  describe('a multi-leg journey', () => {
    function twoLegJourney(): JourneyDetail {
      return baseJourney({
        legs: [
          matchedLeg({ id: 1, originCrs: 'KGX', destinationCrs: 'YRK' }),
          matchedLeg({
            id: 2,
            originCrs: 'YRK',
            destinationCrs: 'NCL',
            trackedTrainState: null,
          }),
        ],
      });
    }

    // 2026-09-22 UX review finding I12/2.6: "the journey is not drawn as
    // a chain" -- number the legs.
    it('numbers each leg "Leg N of M"', async () => {
      vi.mocked(api.getJourney).mockResolvedValue(twoLegJourney());
      await renderPage();
      expect(screen.getByText('Leg 1 of 2')).toBeInTheDocument();
      expect(screen.getByText('Leg 2 of 2')).toBeInTheDocument();
    });

    // Same finding: a connector between legs naming the change point.
    it('draws a connector naming the change point after a non-final leg', async () => {
      vi.mocked(api.getJourney).mockResolvedValue(twoLegJourney());
      await renderPage();
      expect(screen.getByText(/Change at York/)).toBeInTheDocument();
    });

    // The headline fix: the rollup badge alone would show only "Needs a
    // train picked" (leg 2, unmatched outranks delayed) and fully hide
    // leg 1's real 22-minute delay. The per-leg summary line must
    // surface both facts.
    it('surfaces every leg\'s own status in one summary line, not just the worst one', async () => {
      vi.mocked(api.getJourney).mockResolvedValue(twoLegJourney());
      await renderPage();
      expect(screen.getByText('Leg 1 22m late · Leg 2 needs a train picked')).toBeInTheDocument();
      // And the header badge (worst-status rollup) is still present too --
      // this is additive, not a replacement. (I15/2.3: the badge itself
      // now also counts and links to the unmatched leg.)
      expect(screen.getByText('1 leg needs a train')).toBeInTheDocument();
    });
  });
});

function openLeg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
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

function openSuiteTrackedState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-09-22',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'EDB',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: null,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: null,
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    journeyStops: null,
    mayHaveArrived: false,
    sharedGroupCount: 0,
    customName: null,
    ...overrides,
  };
}

function openSuiteJourney(overrides: Partial<JourneyDetail> = {}): JourneyDetail {
  return {
    id: 1,
    customName: null,
    createdAt: '2026-09-22T10:00:00Z',
    legs: [openLeg()],
    isOwner: true,
    shareLink: null,
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
      openSuiteJourney({ legs: [openLeg({ originName: 'London Kings Cross', destinationName: 'Edinburgh' })] }),
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
    vi.mocked(api.getJourney).mockResolvedValue(openSuiteJourney());

    renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id: '1' }) }));
    await screen.findAllByText(/No scheduled trains match this window\./);

    expect(screen.getByRole('heading', { level: 1, name: 'KGX → EDB, 22 Sept 2026' })).toBeInTheDocument();
  });

  it('still prefers a real customName over the computed default', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    vi.mocked(api.getJourney).mockResolvedValue(openSuiteJourney({ customName: 'My commute' }));

    renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id: '1' }) }));
    await screen.findAllByText(/No scheduled trains match this window\./);

    expect(screen.getByRole('heading', { level: 1, name: 'My commute' })).toBeInTheDocument();
  });

  it('spans a multi-leg journey from the first leg\'s origin to the last leg\'s destination', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    vi.mocked(api.getJourney).mockResolvedValue(
      openSuiteJourney({
        legs: [
          openLeg({ id: 1, originCrs: 'KGX', destinationCrs: 'YRK', matchMode: 'manual' }),
          openLeg({ id: 2, originCrs: 'YRK', destinationCrs: 'NCL' }),
        ],
      }),
    );

    renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id: '1' }) }));
    await screen.findAllByText(/No scheduled trains match this window\./);

    expect(screen.getByRole('heading', { level: 1, name: 'KGX → NCL, 22 Sept 2026' })).toBeInTheDocument();
  });
});

describe('JourneyDetailPage "Add a leg" gating (M16)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('hides "Add a leg" while the current (last) leg still needs a train picked', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    vi.mocked(api.getJourney).mockResolvedValue(openSuiteJourney());

    renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id: '1' }) }));
    await screen.findAllByText(/No scheduled trains match this window\./);

    expect(screen.queryByRole('button', { name: 'Add a leg' })).not.toBeInTheDocument();
  });

  it('shows "Add a leg" once the last leg has a matched train', async () => {
    vi.mocked(api.getJourney).mockResolvedValue(
      openSuiteJourney({ legs: [openLeg({ matchMode: 'manual', trackedTrainState: openSuiteTrackedState() })] }),
    );

    renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id: '1' }) }));

    expect(screen.getByRole('button', { name: 'Add a leg' })).toBeInTheDocument();
  });

  it('never shows "Add a leg" to a non-owning group member, matched or not', async () => {
    vi.mocked(api.getJourney).mockResolvedValue(
      openSuiteJourney({
        isOwner: false,
        legs: [openLeg({ matchMode: 'manual', trackedTrainState: openSuiteTrackedState() })],
      }),
    );

    renderWithMantine(await JourneyDetailPage({ params: Promise.resolve({ id: '1' }) }));

    expect(screen.queryByRole('button', { name: 'Add a leg' })).not.toBeInTheDocument();
  });
});

describe('JourneyDetailPage "Track this journey again" button', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  it('renders for the owner', async () => {
    vi.mocked(api.getJourney).mockResolvedValue(baseJourney({ isOwner: true }));
    await renderPage();
    expect(screen.getByRole('button', { name: 'Track this journey again' })).toBeInTheDocument();
  });

  it('renders for a non-owner shared-group viewer too', async () => {
    vi.mocked(api.getJourney).mockResolvedValue(baseJourney({ isOwner: false }));
    await renderPage();
    expect(screen.getByRole('button', { name: 'Track this journey again' })).toBeInTheDocument();
  });
});
