import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyCreationFlow } from './JourneyCreationFlow';
import type { JourneyLegDetail, TripPlanResponse } from '@/lib/types';
import { byVisibleText } from '@/test/routeText';

const pushMock = vi.fn();
const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, refresh: refreshMock }),
  usePathname: () => '/journeys/new',
  useSearchParams: () => new URLSearchParams(''),
}));

function leg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'WAT',
    originName: 'London Waterloo',
    destinationCrs: null,
    destinationName: null,
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'manual',
    trackedTrainState: null,
    legSkip: null,
    ...overrides,
  };
}

function journeyDetailResponse(legs: JourneyLegDetail[]) {
  return new Response(
    JSON.stringify({ id: 99, customName: null, createdAt: '2026-09-22T00:00:00Z', isOwner: true, legs }),
    { status: 200 },
  );
}

/** Routes every fetch this flow (and the two components it composes) can
 * issue, by URL/method -- same idiom `TrackTrainForm.test.tsx`'s own
 * `mockFetchByUrl` and `AddJourneyLegButton.test.tsx` already use, merged
 * for a component that drives both in sequence. */
function mockFetchByUrl(
  options: {
    journeyDetail?: () => Response;
    addLeg?: () => Response;
    tripPlan?: () => Response;
    itinerary?: () => Response;
  } = {},
) {
  return vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (/\/api\/stations\/[A-Za-z]{3}\/departures$/.test(url)) {
      return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    }
    if (url.startsWith('/api/stations?') || url.startsWith('/api/tocs?')) {
      return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    }
    // `PlanTripFlow`'s own `GET /Trips/plan` search -- only the C1
    // regression test (below) exercises the 'plan' mode at all, so a call
    // here with no `tripPlan` override is a genuine test bug, same as the
    // final `throw` below.
    if (url.startsWith('/api/Trips/plan?')) {
      if (!options.tripPlan) throw new Error(`unexpected fetch call: ${url}`);
      return Promise.resolve(options.tripPlan());
    }
    if (url === '/api/Journeys' && init?.method === 'POST') {
      return Promise.resolve(
        new Response(JSON.stringify({ journeyId: 99, legId: 1, trackingId: 42, resolutionStatus: 'pending' }), {
          status: 200,
        }),
      );
    }
    if (url === '/api/Journeys/itinerary' && init?.method === 'POST' && options.itinerary) {
      return Promise.resolve(options.itinerary());
    }
    if (url === '/api/Journeys/99' && (init?.method === undefined || init.method === 'GET')) {
      return Promise.resolve((options.journeyDetail ?? (() => journeyDetailResponse([leg()])))());
    }
    if (url === '/api/Journeys/99/legs' && init?.method === 'POST') {
      return Promise.resolve(
        (options.addLeg ?? (() => new Response(JSON.stringify({ legId: 2, trackingId: null }), { status: 200 })))(),
      );
    }
    throw new Error(`unexpected fetch call: ${url}`);
  });
}

async function createLegOne(fetchMock: ReturnType<typeof vi.fn>) {
  fireEvent.change(screen.getByRole('combobox', { name: /Origin station/ }), { target: { value: 'WAT' } });
  fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
    target: { value: '2026-08-28 18:32:00' },
  });
  fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));
  await waitFor(() =>
    expect(fetchMock.mock.calls.some((call: unknown[]) => call[0] === '/api/Journeys/99')).toBe(true),
  );
}

describe('JourneyCreationFlow', () => {
  beforeEach(() => {
    pushMock.mockClear();
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('starts by rendering the leg-1 form, with no journey yet', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<JourneyCreationFlow initialEntryMode="known" />);
    expect(screen.getByRole('button', { name: /Track this train/ })).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /journey/i })).not.toBeInTheDocument();
  });

  describe('the option it opens on', () => {
    it('opens on the planner, listed first, when nothing asks for another', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<JourneyCreationFlow />);

      const options = within(
        screen.getByRole('radiogroup', { name: 'How do you want to start this journey?' }),
      ).getAllByRole('radio');
      expect(options.map((option) => option.getAttribute('value'))).toEqual(['plan', 'known', 'untracked']);
      expect(screen.getByRole('radio', { name: 'Plan a route for me' })).toBeChecked();
      expect(screen.getByRole('button', { name: 'Find routes' })).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /Track this train/ })).not.toBeInTheDocument();
    });

    it('names the choice by its visible question, for screen readers', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<JourneyCreationFlow />);
      expect(screen.getByRole('radiogroup', { name: 'How do you want to start this journey?' })).toBeInTheDocument();
    });

    it('opens on "I know my route" when asked, on the train form\'s own default half', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<JourneyCreationFlow initialEntryMode="known" />);
      expect(screen.getByRole('radio', { name: 'I know my route' })).toBeChecked();
      expect(screen.getByRole('radio', { name: 'I know the train' })).toBeChecked();
    });

    it('opens "I know my route" on its time window half for knownRouteMode="window"', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<JourneyCreationFlow initialEntryMode="known" knownRouteMode="window" />);
      expect(screen.getByRole('radio', { name: 'I know my route' })).toBeChecked();
      expect(screen.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
    });

    it('opens on "Tube, bus or walk" when asked', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<JourneyCreationFlow initialEntryMode="untracked" />);
      expect(screen.getByRole('radio', { name: 'Tube, bus or walk' })).toBeChecked();
      expect(screen.getByRole('button', { name: 'Start journey' })).toBeInTheDocument();
    });

    it('still switches option after opening on the planner', () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(<JourneyCreationFlow />);
      fireEvent.click(screen.getByText('I know my route'));
      expect(screen.getByRole('button', { name: /Track this train/ })).toBeInTheDocument();
      expect(screen.getByText('Showing the I-know-my-route form.')).toHaveAttribute('aria-live', 'polite');
    });
  });

  it('after leg 1 is created, fetches the journey and offers a Done link to it', async () => {
    const fetchMock = mockFetchByUrl({ journeyDetail: () => journeyDetailResponse([leg()]) });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<JourneyCreationFlow initialEntryMode="known" />);

    await createLegOne(fetchMock);

    expect(await screen.findByRole('link', { name: /view journey/i })).toHaveAttribute('href', '/journeys/99');
    // The leg-1 form is gone -- this is a single continuous flow, not a
    // page that navigates away and back.
    expect(screen.queryByRole('button', { name: /Track this train/ })).not.toBeInTheDocument();
  });

  it('hides "Add a leg" while the only leg is still unmatched', async () => {
    const fetchMock = mockFetchByUrl({
      journeyDetail: () => journeyDetailResponse([leg({ trackedTrainState: null })]),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<JourneyCreationFlow initialEntryMode="known" />);

    await createLegOne(fetchMock);
    await screen.findByRole('link', { name: /view journey/i });

    expect(screen.queryByRole('button', { name: 'Add a leg' })).not.toBeInTheDocument();
  });

  it('offers "Add a leg" once the last leg is matched, and re-fetches the journey after adding one', async () => {
    const matchedLeg = leg({
      destinationCrs: 'CLJ',
      trackedTrainState: {
        id: 1,
        serviceDate: '2026-09-22',
        pinOriginCrs: 'WAT',
        pinDestinationCrs: 'CLJ',
        pinOriginName: null,
        pinDestinationName: null,
        resolutionStatus: 'resolved',
        trainUid: 'P12345',
        trainId: null,
        status: 'en_route',
        lastReportedLocation: null,
        lastEventType: null,
        delayMinutes: 0,
        nextCallingPoint: null,
        etaNext: null,
        etaSource: null,
        scheduleDestinationCrs: 'CLJ',
        scheduleDestinationName: null,
        scheduleCallingPoints: null,
        journeyStops: null,
        mayHaveArrived: false,
        customName: null,
        sharedGroupCount: 0,
      },
    });
    let journeyFetchCount = 0;
    const fetchMock = mockFetchByUrl({
      journeyDetail: () => {
        journeyFetchCount += 1;
        return journeyDetailResponse(journeyFetchCount === 1 ? [matchedLeg] : [matchedLeg, leg({ id: 2 })]);
      },
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<JourneyCreationFlow initialEntryMode="known" />);

    await createLegOne(fetchMock);
    await screen.findByRole('link', { name: /view journey/i });

    fireEvent.click(await screen.findByRole('button', { name: 'Add a leg' }));
    const destination = await screen.findByRole('combobox', { name: 'To station' });
    fireEvent.change(destination, { target: { value: 'CLJ' } });
    fireEvent.change(screen.getByLabelText('Service date'), { target: { value: '2026-09-22' } });
    fireEvent.change(screen.getByLabelText('Earliest departure (optional)'), { target: { value: '09:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add leg' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/99/legs', expect.anything()));
    await waitFor(() => expect(journeyFetchCount).toBe(2));
    // The re-fetched journey now has two legs -- both should be reflected.
    expect(await screen.findAllByText(/Leg \d/)).toHaveLength(2);
    // router.refresh() is the OTHER caller's convention
    // (`app/journeys/[id]/page.tsx`) -- this flow has no server-rendered
    // page to refresh, so it must not be called here.
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('still offers the Done link even when the post-creation journey fetch fails', async () => {
    const fetchMock = mockFetchByUrl({ journeyDetail: () => new Response('boom', { status: 500 }) });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<JourneyCreationFlow initialEntryMode="known" />);

    await createLegOne(fetchMock);

    expect(await screen.findByRole('link', { name: /view journey/i })).toHaveAttribute('href', '/journeys/99');
    expect(screen.queryByRole('button', { name: 'Add a leg' })).not.toBeInTheDocument();
  });

  // Renders the real `JourneyCreationFlow` in plan mode: the whole
  // itinerary is saved in one atomic `POST /Journeys/itinerary` (decision
  // D-15), so there is no partial failure to show, and the parent moves to
  // its post-creation view once the journey exists.
  it('hands off to the post-creation view after the atomic plan-mode save (mode selector -> plan -> save)', async () => {
    const twoSegmentPlan: TripPlanResponse = {
      results: 'fastest',
      segments: [
        {
          originCrs: 'EUS',
          destinationCrs: 'MKC',
          cappedByMaxChanges: false,
          itineraries: [
            {
              legs: [
                {
                  kind: 'train',
                  trainUid: 'C11052',
                  serviceDate: '2026-09-23',
                  originCrs: 'EUS',
                  destinationCrs: 'MKC',
                  scheduledDeparture: '08:00:00',
                  scheduledArrival: '08:50:00',
                  arrivalDayOffset: 0,
                },
              ],
              changeCount: 0,
              totalDurationMinutes: 50,
            },
          ],
        },
        {
          originCrs: 'MKC',
          destinationCrs: 'EDB',
          cappedByMaxChanges: false,
          itineraries: [
            {
              legs: [
                {
                  kind: 'train',
                  trainUid: 'C22000',
                  serviceDate: '2026-09-23',
                  originCrs: 'MKC',
                  destinationCrs: 'EDB',
                  scheduledDeparture: '09:10:00',
                  scheduledArrival: '13:00:00',
                  arrivalDayOffset: 0,
                },
              ],
              changeCount: 0,
              totalDurationMinutes: 230,
            },
          ],
        },
      ],
    };

    const fetchMock = mockFetchByUrl({
      tripPlan: () => new Response(JSON.stringify(twoSegmentPlan), { status: 200 }),
      itinerary: () =>
        new Response(
          JSON.stringify({
            journeyId: 99,
            legId: 1,
            trackingId: 42,
            resolutionStatus: null,
            legs: [
              { legId: 1, trackingId: 42 },
              { legId: 2, trackingId: 43 },
            ],
            createdVia: 'web',
          }),
          { status: 200 },
        ),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<JourneyCreationFlow initialEntryMode="known" />);

    fireEvent.click(screen.getByText('Plan a route for me'));

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'EDB' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText(byVisibleText('08:00 EUS to MKC 08:50'));
    await screen.findByText(byVisibleText('09:10 MKC to EDB 13:00'));
    // Both segments' own single itinerary is the last two radios in DOM
    // order, after the entry-mode selector and the form's own controls.
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 2]!);
    fireEvent.click(radios[radios.length - 1]!);
    fireEvent.click(screen.getByText('Track this journey'));

    expect(await screen.findByRole('link', { name: /view journey/i })).toHaveAttribute('href', '/journeys/99');
  });
});
