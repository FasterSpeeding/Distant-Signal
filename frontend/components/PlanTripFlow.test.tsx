import { screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, expect, it, vi, afterEach } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { PlanTripFlow } from './PlanTripFlow';
import type { TripPlanResponse } from '@/lib/types';

const singleSegmentPlan: TripPlanResponse = {
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
  ],
};

// `PlanTripForm`'s From/To fields are real `Autocomplete`s backed by
// `useSuggestions` (`lib/useSuggestions.ts`), which fires its own debounced
// `searchStations` fetch 250ms after every keystroke -- entirely unrelated
// to this file's own `fetch` mocking of `GET /Trips/plan`/`POST /Journeys`.
// Without stubbing it out, that background fetch would consume one of the
// `mockResolvedValueOnce` slots below (or throw once they're exhausted) the
// moment 250ms of real time elapses during one of this file's own `await
// screen.findByText`/`waitFor` calls, since none of these tests use fake
// timers. `PlanTripForm.test.tsx` itself never hits this because it never
// awaits anything long enough for the debounce to fire; this file's own
// itinerary-selection round trips routinely do. Mocking the module keeps
// every `fetchMock` below scoped to exactly the calls each test cares
// about, sidestepping the interference at its source rather than
// papering over it with a URL-routing fetch mock (contrast
// `TrackTrainForm.test.tsx`'s `mockFetchByUrl`, which takes that heavier
// approach because IT depends on real timers advancing through
// `vi.advanceTimersByTimeAsync` for its own suggestion-dropdown
// assertions -- this file has no such need).
vi.mock('@/lib/suggestions', () => ({
  searchStations: vi.fn().mockResolvedValue([]),
  searchTocs: vi.fn().mockResolvedValue([]),
}));

describe('PlanTripFlow', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('creates the journey via POST /api/Journeys after picking the only itinerary', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(singleSegmentPlan) } as Response)
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
      } as Response);
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    // Not `findByText(/EUS → MKC/)` -- that matches BOTH the segment
    // heading ("EUS → MKC", `PlanTripFlow`'s own `<Text fw={600}>`) and
    // `ItineraryOption`'s own leg summary line ("08:00 EUS → MKC 08:50"),
    // which contains it as a substring; `findByText` throws
    // "Found multiple elements" for an ambiguous match rather than picking
    // one. The itinerary's own line is unique and still proves the plan
    // (and its one itinerary) rendered.
    await screen.findByText('08:00 EUS → MKC 08:50');
    // Not `getByRole('radio')` -- `PlanTripForm`'s own "Fastest"/"Compare
    // options" `SegmentedControl` also renders as a pair of `type="radio"`
    // inputs under the hood (Mantine's implementation, not a styling
    // choice this test controls), so a bare role query matches three
    // radios, not one. `ItineraryOption`'s own `Radio` is the last one in
    // DOM order -- it's the only one below the form -- so it's the last
    // entry in `getAllByRole`'s result.
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]);
    fireEvent.click(screen.getByText('Track this journey'));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 })));
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/Journeys',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ leg: { mode: 'knownTrain', trainUid: 'C11052', serviceDate: '2026-09-23' } }),
      })
    );
  });

  it('shows a plain-text error and does not offer the button when the plan request fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({ ok: false, status: 404, text: () => Promise.resolve('no schedule data published') } as Response)
    );
    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('no schedule data published');
    expect(screen.queryByText('Track this journey')).not.toBeInTheDocument();
  });

  it('names the failing segment when a segment has no itineraries', async () => {
    const noRoutePlan: TripPlanResponse = {
      results: 'fastest',
      segments: [{ originCrs: 'EUS', destinationCrs: 'ZZZ', itineraries: [], cappedByMaxChanges: false }],
    };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(noRoutePlan) } as Response));
    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'ZZZ' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('No route found for EUS → ZZZ.');
  });

  it('creates a leg per train across multiple segments, in order (POST /Journeys then POST /Journeys/{id}/legs)', async () => {
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
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(twoSegmentPlan) } as Response)
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
      } as Response)
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({ legId: 2, trackingId: 8 }) } as Response);
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'EDB' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('08:00 EUS → MKC 08:50');
    await screen.findByText('09:10 MKC → EDB 13:00');
    // Both segments' own single itinerary, the last two radios in DOM order
    // -- see the first test's own comment on why a bare `getByRole('radio')`
    // isn't usable here (the form's SegmentedControl radios come first).
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 2]);
    fireEvent.click(radios[radios.length - 1]);
    fireEvent.click(screen.getByText('Track this journey'));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 })));
    // Call order: [1] the plan GET, [2] `POST /Journeys` for the FIRST
    // train leg (segment 1), [3] `POST /Journeys/{id}/legs` for the
    // SECOND (segment 2) -- never the reverse, and never a second
    // `POST /Journeys`.
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      '/api/Journeys',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ leg: { mode: 'knownTrain', trainUid: 'C11052', serviceDate: '2026-09-23' } }),
      })
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      3,
      '/api/Journeys/42/legs',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ mode: 'knownTrain', trainUid: 'C22000', serviceDate: '2026-09-23' }),
      })
    );
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it('reports which leg failed and still hands off the already-created journey when a later leg’s add-leg call throws a network exception', async () => {
    const twoTrainLegItinerarySegments: TripPlanResponse = {
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
    // Review finding 1: a genuine network-level `fetch` rejection (e.g. a
    // dropped connection -- `TypeError: Failed to fetch` is the real
    // browser message for that) during the leg-2+ loop must be treated the
    // same as an HTTP-error response, NOT escape to the outer catch (which
    // used to report a generic failure and never call `onCreated`, even
    // though leg 1's journey already exists server-side by this point).
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(twoTrainLegItinerarySegments) } as Response)
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
      } as Response)
      .mockRejectedValueOnce(new TypeError('Failed to fetch'));
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'EDB' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('08:00 EUS → MKC 08:50');
    await screen.findByText('09:10 MKC → EDB 13:00');
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 2]);
    fireEvent.click(radios[radios.length - 1]);
    fireEvent.click(screen.getByText('Track this journey'));

    await screen.findByText(
      'Tracked 1 of 2 legs. Adding leg 2 failed: Failed to fetch. You can add it manually from the journey page.'
    );
    // The journey that DOES exist (leg 1 already tracked) must not be
    // withheld from the caller just because leg 2 failed to attach.
    expect(onCreated).toHaveBeenCalledTimes(1);
    expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 }));
    // No retry, and no third leg was ever attempted.
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it('shows a "needs no train" message and never calls POST /Journeys for a plan with no train legs to track', async () => {
    // A plan whose segments carry no itinerary the visitor could ever
    // select (`ItineraryOption` disables the radio for any itinerary with
    // no train leg at all -- Judgment Call 4) can't be reached by picking a
    // train-less itinerary through the UI. The one way `allSegmentsSelected`
    // is honestly satisfied with zero train legs is a plan with NO segments
    // at all (vacuously "every segment has a selection") -- a genuine,
    // type-valid edge case (e.g. an origin===destination query), and it
    // exercises the exact same `trainLegs.length === 0` guard in
    // `handleTrackJourney` that a fixed-link-only itinerary would if it
    // could ever be selected.
    const noSegmentsPlan: TripPlanResponse = { results: 'fastest', segments: [] };
    const fetchMock = vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(noSegmentsPlan) } as Response);
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'EUS' } });
    fireEvent.click(screen.getByText('Find routes'));

    fireEvent.click(await screen.findByText('Track this journey'));

    await screen.findByText('This route needs no train — there is nothing to track.');
    expect(onCreated).not.toHaveBeenCalled();
    // Only the plan GET -- no `POST /Journeys` was ever attempted.
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('renders the cappedByMaxChanges hint for a segment capped at the max-changes limit', async () => {
    const cappedPlan: TripPlanResponse = {
      results: 'fastest',
      segments: [{ ...singleSegmentPlan.segments[0], cappedByMaxChanges: true }],
    };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(cappedPlan) } as Response));
    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('A faster route exists with more changes than shown below.');
  });
});
