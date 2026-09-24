import { act, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, expect, it, vi, afterEach } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { PlanTripFlow } from './PlanTripFlow';
import type { TripPlanResponse } from '@/lib/types';

// `LoginPromptModal` (rendered unconditionally, per its own doc comment)
// pulls in `useLoginHref`, which calls `usePathname`/`useSearchParams` --
// real Next.js navigation hooks that throw outside a router context. Same
// mock `TrackTrainForm.test.tsx` already needs for the exact same reason.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/journeys/new',
  useSearchParams: () => new URLSearchParams(''),
}));

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
//
// Final-review fix (C2): plain `async` functions, NOT `vi.fn().
// mockResolvedValue(...)`. This file's own `afterEach(() =>
// vi.restoreAllMocks())` below strips a `vi.fn()`'s mock IMPLEMENTATION
// after the first test runs, while leaving the `vi.fn()` itself in place
// as the module's export -- from the second test onward `searchStations`/
// `searchTocs` returned `undefined` instead of a promise. `useSuggestions`'
// 250ms debounce timer from an EARLIER test's field interaction can still
// be pending when it fires during a LATER test (test bodies run faster
// than 250ms, but real time keeps advancing across tests since none of
// them use fake timers), calling `.then()` on that `undefined` and
// throwing an uncaught `TypeError` that doesn't fail the individual `it()`
// but crashes the whole `npm test` process (reproduced ~1-in-3 runs when
// running multiple test files together). A plain `async () => []` has no
// mock-implementation state for `restoreAllMocks` to strip, so it keeps
// returning a real, resolved promise for the lifetime of this module,
// across every test in this file.
vi.mock('@/lib/suggestions', () => ({
  searchStations: async () => [],
  searchTocs: async () => [],
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
        body: JSON.stringify({
          leg: { mode: 'knownTrain', trainUid: 'C11052', serviceDate: '2026-09-23', originCrs: 'EUS', destinationCrs: 'MKC' },
        }),
      })
    );
  });

  it('omits originCrs/destinationCrs entirely (not null) when the itinerary leg has no CRS for either end', async () => {
    // `TripPlanLeg`'s train variant types `originCrs`/`destinationCrs` as
    // `string | null` -- a real `GET /Trips/plan` response can carry `null`
    // for a TIPLOC with no CRS mapping (`crs_for_tiploc`,
    // `crates/api/src/data/trip_planning_itinerary.rs`). The conditional
    // spread in `PlanTripFlow.tsx` must omit the key entirely on that
    // `null` branch, not send an explicit `null` -- every other test in
    // this file only ever exercises the non-null branch, since their
    // fixtures always carry real CRS strings.
    const nullOverridesPlan: TripPlanResponse = {
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
                  originCrs: null,
                  destinationCrs: null,
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
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(nullOverridesPlan) } as Response)
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

    // `ItineraryOption.tsx` renders `leg.originCrs ?? '?'`/`leg.destinationCrs
    // ?? '?'` for its own summary line -- a `null` leg CRS shows as `?`, not
    // the segment's own `EUS`/`MKC` heading (that's a separate `<Text>` line
    // above, from `TripPlanSegment.originCrs`/`destinationCrs`, which are
    // always non-null strings).
    await screen.findByText('08:00 ? → ? 08:50');
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]);
    fireEvent.click(screen.getByText('Track this journey'));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 })));
    // No `originCrs`/`destinationCrs` keys at all -- not present as `null`.
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/Journeys',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          leg: { mode: 'knownTrain', trainUid: 'C11052', serviceDate: '2026-09-23' },
        }),
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
        body: JSON.stringify({
          leg: { mode: 'knownTrain', trainUid: 'C11052', serviceDate: '2026-09-23', originCrs: 'EUS', destinationCrs: 'MKC' },
        }),
      })
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      3,
      '/api/Journeys/42/legs',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          mode: 'knownTrain',
          trainUid: 'C22000',
          serviceDate: '2026-09-23',
          originCrs: 'MKC',
          destinationCrs: 'EDB',
        }),
      })
    );
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it("sends the train leg's own origin/destination, not the segment's, for a leg that boards/alights mid-route", async () => {
    // The plan's own Birmingham->Glasgow/Crewe->Preston example, made
    // concrete: a single segment advertised as BHM -> GLC whose one
    // itinerary's train leg actually boards at Crewe and alights at
    // Preston (a transfer leg before/after the train leg in a real
    // itinerary is what would produce this -- irrelevant to this test,
    // which only needs the train leg's own origin/destination to differ
    // from the segment's). The committed `knownTrain` request must carry
    // the LEG's own CRE/PRE, never the segment's BHM/GLC.
    const midRoutePlan: TripPlanResponse = {
      results: 'fastest',
      segments: [
        {
          originCrs: 'BHM',
          destinationCrs: 'GLC',
          cappedByMaxChanges: false,
          itineraries: [
            {
              legs: [
                {
                  kind: 'train',
                  trainUid: 'X12345',
                  serviceDate: '2026-09-23',
                  originCrs: 'CRE',
                  destinationCrs: 'PRE',
                  scheduledDeparture: '10:00:00',
                  scheduledArrival: '11:15:00',
                  arrivalDayOffset: 0,
                },
              ],
              changeCount: 0,
              totalDurationMinutes: 75,
            },
          ],
        },
      ],
    };
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(midRoutePlan) } as Response)
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
      } as Response);
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'BHM' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'GLC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('10:00 CRE → PRE 11:15');
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]);
    fireEvent.click(screen.getByText('Track this journey'));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 })));
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/Journeys',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          leg: { mode: 'knownTrain', trainUid: 'X12345', serviceDate: '2026-09-23', originCrs: 'CRE', destinationCrs: 'PRE' },
        }),
      })
    );
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
    // C1 (final-review fix): `onCreated` must NOT fire yet -- calling it
    // here, before the visitor has had a chance to actually read the
    // message above, is exactly the unmountable-by-construction bug this
    // fixes (see `PlanTripFlow.tsx`'s own doc comment). The message and
    // the hand-off button must coexist on screen first.
    expect(onCreated).not.toHaveBeenCalled();
    expect(screen.getByText('Tracked 1 of 2 legs. Adding leg 2 failed: Failed to fetch. You can add it manually from the journey page.')).toBeInTheDocument();
    expect(screen.queryByText('Track this journey')).not.toBeInTheDocument();

    // Only once the visitor clicks through does the already-created
    // journey (leg 1 already tracked) get handed off -- never withheld,
    // just deferred.
    fireEvent.click(screen.getByText('Continue to your journey'));
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

  it('shows the login prompt (not a raw error) on a 401 from the initial POST /Journeys, and never calls onCreated', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(singleSegmentPlan) } as Response)
      .mockResolvedValueOnce(new Response('no session', { status: 401 }));
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('08:00 EUS → MKC 08:50');
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]);
    fireEvent.click(screen.getByText('Track this journey'));

    // Same `LoginPromptModal` copy/pattern `TrackTrainForm.tsx` already
    // uses for its own `POST /Journeys` 401 -- not the raw "no session"
    // body text.
    expect(await screen.findByText('Log in to track this journey.')).toBeInTheDocument();
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('shows the login prompt and still offers a hand-off when a later leg’s add-leg call 401s', async () => {
    const twoSegmentPlan: TripPlanResponse = {
      results: 'fastest',
      segments: [
        singleSegmentPlan.segments[0],
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
      .mockResolvedValueOnce(new Response('no session', { status: 401 }));
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

    expect(await screen.findByText('Log in to track this journey.')).toBeInTheDocument();
    await screen.findByText(/Tracked 1 of 2 legs\. Your session expired before leg 2 could be added\./);
    expect(onCreated).not.toHaveBeenCalled();

    // The already-created journey (leg 1) must still be reachable, exactly
    // like any other partial-failure -- a 401 mid-sequence is one more
    // reason a leg can fail to attach, not a special dead end.
    fireEvent.click(screen.getByText('Continue to your journey'));
    expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 }));
  });

  it('shows a loading label on the search button while a plan search is pending, then clears it', async () => {
    let resolveFetch: (value: Response) => void = () => {};
    const pending = new Promise<Response>(resolve => {
      resolveFetch = resolve;
    });
    const fetchMock = vi.fn().mockReturnValueOnce(pending);
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });

    expect(screen.queryByText('Searching…')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('Searching…');

    resolveFetch({ ok: true, json: () => Promise.resolve(singleSegmentPlan) } as Response);

    await screen.findByText('Find routes');
    expect(screen.queryByText('Searching…')).not.toBeInTheDocument();
    await screen.findByText('08:00 EUS → MKC 08:50');
  });

  it('discards a stale plan response that resolves after a newer one', async () => {
    // Regression coverage for the `searchRequestId` guard itself
    // (`PlanTripFlow.tsx`'s own doc comment) -- kept as defense-in-depth
    // even though the search button is now disabled while `searching`
    // (see the "does not double-fetch" test below for that), by driving
    // TWO overlapping searches the way the guard is actually built to
    // survive: each call captures its own request id, and only the most
    // recent one is allowed to apply its result, regardless of resolution
    // order. Simulated here via two full render passes (`unmount` + a
    // fresh `render`) rather than two rapid clicks on the same button,
    // since the latter is now a genuine no-op by design (`PlanTripForm`'s
    // `disabled` prop) and can no longer produce two in-flight requests.
    const firstPlan: TripPlanResponse = {
      results: 'fastest',
      segments: [{ originCrs: 'AAA', destinationCrs: 'BBB', cappedByMaxChanges: false, itineraries: [] }],
    };
    const secondPlan: TripPlanResponse = {
      results: 'fastest',
      segments: [{ originCrs: 'CCC', destinationCrs: 'DDD', cappedByMaxChanges: false, itineraries: [] }],
    };

    let resolveFirst: (value: Response) => void = () => {};
    let resolveSecond: (value: Response) => void = () => {};
    const firstResponse = new Promise<Response>(resolve => {
      resolveFirst = resolve;
    });
    const secondResponse = new Promise<Response>(resolve => {
      resolveSecond = resolve;
    });

    const fetchMock = vi.fn().mockReturnValueOnce(firstResponse).mockReturnValueOnce(secondResponse);
    vi.stubGlobal('fetch', fetchMock);

    const { unmount } = renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'AAA' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'BBB' } });
    fireEvent.click(screen.getByText('Find routes'));
    await screen.findByText('Searching…');
    // Unmounting mid-search does not cancel the in-flight `fetch` -- the
    // stale first request is still going to resolve later, same as it
    // would if a second, later click had issued it instead.
    unmount();

    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'CCC' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'DDD' } });
    fireEvent.click(screen.getByText('Find routes'));
    expect(fetchMock).toHaveBeenCalledTimes(2);

    // Resolve the NEWER (second) request first, then the STALE (first,
    // slower) one -- the stale one must not clobber the newer result once
    // it finally resolves.
    resolveSecond({ ok: true, json: () => Promise.resolve(secondPlan) } as Response);
    await screen.findByText('CCC → DDD');

    // Flush the stale response's own `.then()`/`.finally()` chain (a real
    // `setTimeout(0)`, not `waitFor` -- there's nothing to poll for since
    // the CORRECT outcome is that nothing further changes).
    await act(async () => {
      resolveFirst({ ok: true, json: () => Promise.resolve(firstPlan) } as Response);
      await new Promise(resolve => setTimeout(resolve, 0));
    });
    expect(screen.queryByText('AAA → BBB')).not.toBeInTheDocument();
    expect(screen.getByText('CCC → DDD')).toBeInTheDocument();
  });

  it('does not double-fetch GET /Trips/plan when the search button is clicked rapidly twice', async () => {
    // Final-review follow-up: the request-id guard above already made an
    // overlapping response harmless, but nothing stopped a second click
    // while a search was in flight from firing another real
    // `GET /Trips/plan` -- wasted backend pathfinding work per extra
    // click. Mirrors `JourneyLegCandidates.test.tsx`'s own "does not
    // double-fetch page 2 when Load more is clicked rapidly" test:
    // several synchronous clicks on the SAME button reference before the
    // in-flight request resolves, then assert only one fetch actually went
    // out.
    let resolveFetch: (value: Response) => void = () => {};
    const pending = new Promise<Response>(resolve => {
      resolveFetch = resolve;
    });
    const fetchMock = vi.fn().mockReturnValueOnce(pending);
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });

    const button = screen.getByText('Find routes');
    fireEvent.click(button);
    fireEvent.click(button);
    fireEvent.click(button);

    await screen.findByText('Searching…');
    resolveFetch({ ok: true, json: () => Promise.resolve(singleSegmentPlan) } as Response);

    await screen.findByText('08:00 EUS → MKC 08:50');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
