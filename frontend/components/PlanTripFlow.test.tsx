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
});
