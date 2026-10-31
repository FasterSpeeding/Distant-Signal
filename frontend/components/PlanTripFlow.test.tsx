import { act, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, expect, it, vi, afterEach } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { PlanTripFlow } from './PlanTripFlow';
import { resetStationGroupsCache } from '@/lib/stationGroups';
import type { PlannerOperatorCatalogue, TripPlanResponse } from '@/lib/types';
import type { PlanFormInitial } from '@/lib/tripPlanUrl';
import { byVisibleText } from '@/test/routeText';
import { RAIL_TRAM_RAIL } from '@/test/transitPlanFixtures';

// `LoginPromptModal` (rendered unconditionally, per its own doc comment)
// pulls in `useLoginHref`, which calls `usePathname`/`useSearchParams` --
// real Next.js navigation hooks that throw outside a router context. Same
// mock `TrackTrainForm.test.tsx` already needs for the exact same reason.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/plan',
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
// `mock`-prefixed (per Vitest's own hoisting exception -- a `vi.mock`
// factory can only reference outer variables named `mock*`), and read
// live rather than via `mockResolvedValue(...)`: same C2 rationale as
// `searchStations`/`searchTocs` below -- a plain function that reads
// mutable state at call time has no mock-implementation state for this
// file's own `afterEach(() => vi.restoreAllMocks())` to strip, so it keeps
// resolving correctly for the lifetime of this module, across every test.
// Empty by default (no test needs a resolved name unless it populates
// this first), and cleared in `afterEach` below so one test's names never
// leak into the next.
const mockStationNames = new Map<string, string>();

vi.mock('@/lib/suggestions', () => ({
  searchStations: async () => [],
  searchPlannerLocations: async () => [],
  searchTocs: async () => [],
  getStationNames: async () => new Map(mockStationNames),
}));

// The From and To search also asks the transit networks for tram stations
// (`GET /api/transit/networks`), which would take one of the `fetch`
// mocks below for the same debounce reason.
vi.mock('@/lib/plannerEnds', () => ({ searchPlannerEnds: async () => [] }));

describe('PlanTripFlow', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    mockStationNames.clear();
  });

  // The bug this fixes: `GET /Trips/plan` never sends a `*Name` sibling
  // field for its bare CRS codes (unlike every other station-bearing
  // response in this app), so the segment heading, the "no route found"
  // alert, and each itinerary's own leg summary all used to render ONLY
  // the raw code -- unreadable to anyone who doesn't already know UK
  // station codes by heart. `PlanTripFlow` now resolves every code the
  // plan mentions itself (`getStationNames`, mocked here via
  // `mockStationNames`) and renders `CODE · Name` wherever a bare code
  // used to stand alone.
  it('shows resolved station names (CODE · Name) in the segment heading and itinerary leg summary', async () => {
    mockStationNames.set('EUS', 'London Euston');
    mockStationNames.set('MKC', 'Milton Keynes Central');
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(singleSegmentPlan) }));

    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    // The segment heading (an exact match -- unlike the bare-code case
    // above, this full string is unique: the itinerary line below embeds
    // it inside "08:00 ... 08:50", which is a DIFFERENT full string, not
    // an ambiguous substring match).
    await screen.findByText(byVisibleText('EUS · London Euston to MKC · Milton Keynes Central'));
    // The itinerary's own leg summary line, same names.
    await screen.findByText(byVisibleText('08:00 EUS · London Euston to MKC · Milton Keynes Central 08:50'));
  });

  it('falls back to bare codes in the segment heading/leg summary when no name resolves', async () => {
    // `mockStationNames` is empty by default (cleared in `afterEach`) --
    // this is the "lookup found nothing" case, distinct from the bug this
    // fixes (which was "no lookup was ever attempted").
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(singleSegmentPlan) }));

    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText(byVisibleText('EUS to MKC'));
    await screen.findByText(byVisibleText('08:00 EUS to MKC 08:50'));
  });

  it('creates the journey via POST /api/Journeys/itinerary after picking the only itinerary', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(singleSegmentPlan) })
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
      });
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    // Not `findByText(byVisibleText(/EUS to MKC/))` -- that matches BOTH the segment
    // heading ("EUS to MKC", `PlanTripFlow`'s own `<Text fw={600}>`) and
    // `ItineraryOption`'s own leg summary line ("08:00 EUS to MKC 08:50"),
    // which contains it as a substring; `findByText` throws
    // "Found multiple elements" for an ambiguous match rather than picking
    // one. The itinerary's own line is unique and still proves the plan
    // (and its one itinerary) rendered.
    await screen.findByText(byVisibleText('08:00 EUS to MKC 08:50'));
    // Not `getByRole('radio')` -- `PlanTripForm`'s own "Fastest"/"Compare
    // options" `SegmentedControl` also renders as a pair of `type="radio"`
    // inputs under the hood (Mantine's implementation, not a styling
    // choice this test controls), so a bare role query matches three
    // radios, not one. `ItineraryOption`'s own `Radio` is the last one in
    // DOM order -- it's the only one below the form -- so it's the last
    // entry in `getAllByRole`'s result.
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]!);
    fireEvent.click(screen.getByText('Track this journey'));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 })));
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/Journeys/itinerary',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          legs: [{ trainUid: 'C11052', serviceDate: '2026-09-23', originCrs: 'EUS', destinationCrs: 'MKC' }],
        }),
      }),
    );
    // A fresh Idempotency-Key per click, so a retried request cannot save
    // the journey twice.
    const init = fetchMock.mock.calls[1]![1] as RequestInit;
    const headers = init.headers as Record<string, string>;
    expect(headers['Idempotency-Key']).toMatch(/\S{8,}/);
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
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(nullOverridesPlan) })
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
      });
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
    await screen.findByText(byVisibleText('08:00 ? to ? 08:50'));
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]!);
    fireEvent.click(screen.getByText('Track this journey'));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 })));
    // No `originCrs`/`destinationCrs` keys at all -- not present as `null`.
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/Journeys/itinerary',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ legs: [{ trainUid: 'C11052', serviceDate: '2026-09-23' }] }),
      }),
    );
  });

  it('shows a plain-text error and does not offer the button when the plan request fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: false,
        status: 404,
        text: () => Promise.resolve('no schedule data published'),
      }),
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
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(noRoutePlan) }));
    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'ZZZ' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText(byVisibleText('No route found for EUS to ZZZ.'));
  });

  it('says which constraint left a segment with no itineraries', async () => {
    const noRoutePlan: TripPlanResponse = {
      results: 'fastest',
      segments: [
        {
          originCrs: 'EUS',
          destinationCrs: 'MKC',
          itineraries: [],
          cappedByMaxChanges: false,
          noResultReason: {
            constraint: 'departAfter',
            values: ['23:30'],
            message: 'No itinerary from EUS to MKC departs after 23:30 on 2026-09-29; the last one leaves at 23:10.',
          },
        },
      ],
    };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(noRoutePlan) }));
    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText(/the last one leaves at 23:10/);
  });

  it('saves every train leg across multiple segments, in order, in one POST /Journeys/itinerary', async () => {
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
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(twoSegmentPlan) })
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
      });
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'EDB' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText(byVisibleText('08:00 EUS to MKC 08:50'));
    await screen.findByText(byVisibleText('09:10 MKC to EDB 13:00'));
    // Both segments' own single itinerary, the last two radios in DOM order
    // -- see the first test's own comment on why a bare `getByRole('radio')`
    // isn't usable here (the form's SegmentedControl radios come first).
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 2]!);
    fireEvent.click(radios[radios.length - 1]!);
    fireEvent.click(screen.getByText('Track this journey'));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 })));
    // One request after the plan GET, carrying every train leg in order:
    // the journey and its legs are created together or not at all.
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      '/api/Journeys/itinerary',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          legs: [
            { trainUid: 'C11052', serviceDate: '2026-09-23', originCrs: 'EUS', destinationCrs: 'MKC' },
            { trainUid: 'C22000', serviceDate: '2026-09-23', originCrs: 'MKC', destinationCrs: 'EDB' },
          ],
        }),
      }),
    );
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  // Spec 2026-10-20 §2, follow-up: a cross-London Tube transfer is saved
  // as an untracked leg with its mode, station names and planned times,
  // instead of being dropped.
  it('saves a Tube transfer between two trains as an untracked leg', async () => {
    const plan: TripPlanResponse = {
      results: 'fastest',
      segments: [
        {
          originCrs: 'MKC',
          destinationCrs: 'YRK',
          cappedByMaxChanges: false,
          itineraries: [
            {
              legs: [
                {
                  kind: 'train',
                  trainUid: 'A11111',
                  serviceDate: '2026-10-21',
                  originCrs: 'MKC',
                  destinationCrs: 'EUS',
                  scheduledDeparture: '09:00:00',
                  scheduledArrival: '09:50:00',
                  arrivalDayOffset: 0,
                },
                { kind: 'transfer', mode: 'TUBE', originCrs: 'EUS', destinationCrs: 'KGX', minutes: 20 },
                {
                  kind: 'train',
                  trainUid: 'B22222',
                  serviceDate: '2026-10-21',
                  originCrs: 'KGX',
                  destinationCrs: 'YRK',
                  scheduledDeparture: '10:30:00',
                  scheduledArrival: '12:30:00',
                  arrivalDayOffset: 0,
                },
              ],
              changeCount: 1,
              totalDurationMinutes: 210,
            },
          ],
        },
      ],
    };
    mockStationNames.set('EUS', 'London Euston');
    mockStationNames.set('KGX', 'London Kings Cross');
    const fetchMock = vi.fn((input: RequestInfo | URL, _init?: RequestInit) => {
      const url = String(input);
      if (url.startsWith('/api/Journeys/itinerary')) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
        });
      }
      return Promise.resolve({ ok: true, json: () => Promise.resolve(plan) });
    });
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'MKC' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'YRK' } });
    fireEvent.click(screen.getByText('Find routes'));
    await waitFor(() => expect(screen.getAllByRole('radio').length).toBeGreaterThan(0));
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]!);
    // Station names arrive with the plan; wait for them before saving.
    await screen.findAllByText(/London Euston/);
    fireEvent.click(screen.getByText('Track this journey'));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 })));
    const saved = fetchMock.mock.calls.find(([input]) => String(input).startsWith('/api/Journeys/itinerary'));
    const body = JSON.parse(String(saved?.[1]?.body)) as { legs: unknown[] };
    expect(body.legs).toEqual([
      { trainUid: 'A11111', serviceDate: '2026-10-21', originCrs: 'MKC', destinationCrs: 'EUS' },
      {
        kind: 'untracked',
        transitMode: 'tube',
        originName: 'London Euston',
        destinationName: 'London Kings Cross',
        plannedDeparture: '2026-10-21T08:50:00.000Z',
        plannedArrival: '2026-10-21T09:10:00.000Z',
      },
      { trainUid: 'B22222', serviceDate: '2026-10-21', originCrs: 'KGX', destinationCrs: 'YRK' },
    ]);
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
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(midRoutePlan) })
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
      });
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'BHM' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'GLC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText(byVisibleText('10:00 CRE to PRE 11:15'));
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]!);
    fireEvent.click(screen.getByText('Track this journey'));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 })));
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/Journeys/itinerary',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          legs: [{ trainUid: 'X12345', serviceDate: '2026-09-23', originCrs: 'CRE', destinationCrs: 'PRE' }],
        }),
      }),
    );
  });

  it('shows the failure, creates nothing and keeps the button when the save fails', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(singleSegmentPlan) })
      .mockRejectedValueOnce(new TypeError('Failed to fetch'))
      .mockResolvedValueOnce(
        new Response("You're already tracking 100 upcoming trains, which is the maximum. Remove some to make room.", {
          status: 400,
        }),
      );
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText(byVisibleText('08:00 EUS to MKC 08:50'));
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]!);

    // A network failure: the generic sentence, and the button stays, since
    // the atomic save created nothing.
    fireEvent.click(screen.getByText('Track this journey'));
    expect(await screen.findByText("Couldn't track this journey")).toBeInTheDocument();
    expect(onCreated).not.toHaveBeenCalled();
    expect(screen.getByText('Track this journey')).toBeInTheDocument();

    // An API refusal: its own sentence.
    fireEvent.click(screen.getByText('Track this journey'));
    expect(
      await screen.findByText(
        "You're already tracking 100 upcoming trains, which is the maximum. Remove some to make room.",
      ),
    ).toBeInTheDocument();
    expect(onCreated).not.toHaveBeenCalled();
    // Each click is a new attempt with its own key.
    const keys = fetchMock.mock.calls
      .slice(1)
      .map((call) => ((call[1] as RequestInit).headers as Record<string, string>)['Idempotency-Key']);
    expect(new Set(keys).size).toBe(2);
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
    const fetchMock = vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(noSegmentsPlan) });
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'EUS' } });
    fireEvent.click(screen.getByText('Find routes'));

    fireEvent.click(await screen.findByText('Track this journey'));

    await screen.findByText('This route needs no train, so there is nothing to track.');
    expect(onCreated).not.toHaveBeenCalled();
    // Only the plan GET -- no `POST /Journeys` was ever attempted.
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('renders the cappedByMaxChanges hint for a segment capped at the max-changes limit', async () => {
    const cappedPlan: TripPlanResponse = {
      results: 'fastest',
      segments: [{ ...singleSegmentPlan.segments[0]!, cappedByMaxChanges: true }],
    };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(cappedPlan) }));
    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('A faster route exists with more changes than shown below.');
  });

  it.each([
    [true, true],
    [false, false],
    [undefined, false],
  ])('provisional %s shows the timetable note: %s', async (provisional, shown) => {
    const plan: TripPlanResponse =
      provisional === undefined
        ? singleSegmentPlan
        : { ...singleSegmentPlan, provisional, provisionalFrom: '2026-10-16' };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(plan) }));
    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText(byVisibleText('EUS to MKC'));
    if (shown) {
      expect(screen.getByText('Timetable may change')).toBeInTheDocument();
    } else {
      expect(screen.queryByText('Timetable may change')).not.toBeInTheDocument();
    }
  });

  it('shows the login prompt (not a raw error) on a 401 from POST /Journeys/itinerary, and never calls onCreated', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(singleSegmentPlan) })
      .mockResolvedValueOnce(new Response('no session', { status: 401 }));
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText(byVisibleText('08:00 EUS to MKC 08:50'));
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]!);
    fireEvent.click(screen.getByText('Track this journey'));

    // Same `LoginPromptModal` copy/pattern `TrackTrainForm.tsx` already
    // uses for its own `POST /Journeys` 401 -- not the raw "no session"
    // body text.
    expect(await screen.findByText('Log in to track this journey.')).toBeInTheDocument();
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('shows a loading label on the search button while a plan search is pending, then clears it', async () => {
    let resolveFetch: (value: Response) => void = () => {};
    const pending = new Promise<Response>((resolve) => {
      resolveFetch = resolve;
    });
    const fetchMock = vi.fn().mockReturnValueOnce(pending);
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });

    expect(screen.queryByText('Searching')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('Searching');

    resolveFetch({ ok: true, json: () => Promise.resolve(singleSegmentPlan) } as Response);

    await screen.findByText('Find routes');
    expect(screen.queryByText('Searching')).not.toBeInTheDocument();
    await screen.findByText(byVisibleText('08:00 EUS to MKC 08:50'));
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
    const firstResponse = new Promise<Response>((resolve) => {
      resolveFirst = resolve;
    });
    const secondResponse = new Promise<Response>((resolve) => {
      resolveSecond = resolve;
    });

    const fetchMock = vi.fn().mockReturnValueOnce(firstResponse).mockReturnValueOnce(secondResponse);
    vi.stubGlobal('fetch', fetchMock);

    const { unmount } = renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'AAA' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'BBB' } });
    fireEvent.click(screen.getByText('Find routes'));
    await screen.findByText('Searching');
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
    await screen.findByText(byVisibleText('CCC to DDD'));

    // Flush the stale response's own `.then()`/`.finally()` chain (a real
    // `setTimeout(0)`, not `waitFor` -- there's nothing to poll for since
    // the CORRECT outcome is that nothing further changes).
    await act(async () => {
      resolveFirst({ ok: true, json: () => Promise.resolve(firstPlan) } as Response);
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(screen.queryByText(byVisibleText('AAA to BBB'))).not.toBeInTheDocument();
    expect(screen.getByText(byVisibleText('CCC to DDD'))).toBeInTheDocument();
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
    const pending = new Promise<Response>((resolve) => {
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

    await screen.findByText('Searching');
    resolveFetch({ ok: true, json: () => Promise.resolve(singleSegmentPlan) } as Response);

    await screen.findByText(byVisibleText('08:00 EUS to MKC 08:50'));
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});

describe('PlanTripFlow advanced options', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    mockStationNames.clear();
    window.history.replaceState(null, '', '/');
  });

  const viaPlan: TripPlanResponse = {
    results: 'fastest',
    via: ['STA', 'CRE'],
    segments: [
      {
        originCrs: 'EUS',
        destinationCrs: 'PRE',
        cappedByMaxChanges: false,
        itineraries: [
          {
            legs: [
              {
                kind: 'train',
                trainUid: 'C11052',
                serviceDate: '2026-10-08',
                originCrs: 'EUS',
                destinationCrs: 'CRE',
                scheduledDeparture: '08:00:00',
                scheduledArrival: '09:30:00',
                arrivalDayOffset: 0,
              },
              {
                kind: 'train',
                trainUid: 'C22000',
                serviceDate: '2026-10-08',
                originCrs: 'CRE',
                destinationCrs: 'PRE',
                scheduledDeparture: '09:40:00',
                scheduledArrival: '10:20:00',
                arrivalDayOffset: 0,
              },
            ],
            changeCount: 1,
            totalDurationMinutes: 140,
          },
        ],
      },
    ],
    journeys: [
      {
        changeCount: 1,
        departure: { time: '08:00', dayOffset: 0 },
        arrival: { time: '10:20', dayOffset: 0 },
        totalDurationMinutes: 140,
        viaSatisfiedBy: [
          { crs: 'STA', segment: 0, leg: 0, how: 'pass' },
          { crs: 'CRE', segment: 0, leg: 0, how: 'call' },
        ],
      },
    ],
  };

  async function search(fetchMock: ReturnType<typeof vi.fn>, props: Partial<Parameters<typeof PlanTripFlow>[0]> = {}) {
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} {...props} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'PRE' } });
    fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
  }

  it('names the London terminal a group "Call at" stop used (2026-10-07)', async () => {
    resetStationGroupsCache();
    mockStationNames.set('KGX', 'London Kings Cross');
    mockStationNames.set('EUS', 'London Euston');
    mockStationNames.set('PRE', 'Preston');
    const train = (originCrs: string, destinationCrs: string) => ({
      kind: 'train' as const,
      trainUid: 'G1',
      serviceDate: '2026-10-08',
      originCrs,
      destinationCrs,
      scheduledDeparture: '08:00:00',
      scheduledArrival: '09:00:00',
      arrivalDayOffset: 0,
    });
    const groupPlan: TripPlanResponse = {
      results: 'fastest',
      via: [],
      segments: [
        {
          originCrs: 'EUS',
          destinationCrs: 'group:LON',
          cappedByMaxChanges: false,
          itineraries: [{ legs: [train('EUS', 'KGX')], changeCount: 0, totalDurationMinutes: 60 }],
        },
        {
          originCrs: 'group:LON',
          destinationCrs: 'PRE',
          cappedByMaxChanges: false,
          itineraries: [{ legs: [train('KGX', 'PRE')], changeCount: 0, totalDurationMinutes: 60 }],
        },
      ],
      journeys: [
        {
          changeCount: 1,
          departure: { time: '08:00', dayOffset: 0 },
          arrival: { time: '09:00', dayOffset: 0 },
          totalDurationMinutes: 120,
          waypointSatisfiedBy: [{ crs: 'group:LON', matchedCrs: 'KGX', segment: 0, how: 'call' }],
        },
      ],
    };
    const groups = {
      groups: [
        {
          group: 'LON',
          code: 'group:LON',
          name: 'London Terminals',
          members: [{ crs: 'KGX', name: 'London Kings Cross' }],
        },
      ],
    };
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(groupPlan) })
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(groups) });
    await search(fetchMock);
    expect(await screen.findByText('Stops at London Kings Cross (one of the London Terminals)')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenLastCalledWith('/api/Trips/station-groups', {});
    // The segment headings name the group.
    expect(screen.getAllByText(byVisibleText(/Any of the London Terminals \(1 station\)/)).length).toBeGreaterThan(0);
    resetStationGroupsCache();
  });

  it('says how each via was passed: through without stopping, or calling there', async () => {
    mockStationNames.set('STA', 'Stafford');
    mockStationNames.set('CRE', 'Crewe');
    await search(vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(viaPlan) }));
    expect(await screen.findByText('Passes through Stafford without stopping')).toBeInTheDocument();
    expect(screen.getByText('Calls at Crewe')).toBeInTheDocument();
  });

  it('sends the restored advanced options to GET /Trips/plan', async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(viaPlan) });
    await search(fetchMock, { initialQuery: { viaCrs: ['STA', 'CRE'], avoidStopCrs: ['WVH'], maxChanges: 5 } });
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const url = new URL(String(fetchMock.mock.calls[0]![0]), 'http://localhost');
    expect(url.searchParams.get('via')).toBe('STA,CRE');
    expect(url.searchParams.get('avoidStop')).toBe('WVH');
    expect(url.searchParams.get('maxChanges')).toBe('5');
  });

  it('writes the search into the address bar on /plan', async () => {
    window.history.replaceState(null, '', '/plan?origin=EUS');
    await search(vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(viaPlan) }), {
      initialQuery: { viaCrs: ['STA'] },
      syncUrl: true,
    });
    await waitFor(() => expect(window.location.search).toContain('via=STA'));
    expect(window.location.pathname).toBe('/plan');
    expect(window.location.search).toContain('origin=EUS');
    expect(window.location.search).toContain('destination=PRE');
  });

  it('leaves the address bar alone without syncUrl', async () => {
    window.history.replaceState(null, '', '/journeys/42');
    const fetchMock = vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(viaPlan) });
    await search(fetchMock);
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    expect(window.location.search).toBe('');
  });

  it('turns the search-size 400 into advice', async () => {
    await search(
      vi.fn().mockResolvedValueOnce({
        ok: false,
        status: 400,
        text: () =>
          Promise.resolve(
            'results=options with 4 waypoints, 3 vias and maxChanges=6 is too large a search ((waypoints + 1) * (2 * vias + 1) * (maxChanges + 2) = 280, at most 252); use fewer waypoints or vias, a lower maxChanges, or results=fastest',
          ),
      }),
    );
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('the search is too large');
    expect(alert).toHaveTextContent('Switch Results to Fastest');
    expect(alert).toHaveTextContent('Remove some stops to call at, or some "Pass through" stations');
    expect(alert).not.toHaveTextContent('(waypoints + 1)');
  });

  it('explains a via no route passes, with what to try', async () => {
    mockStationNames.set('STA', 'Stafford');
    const noVia: TripPlanResponse = {
      results: 'fastest',
      via: ['STA'],
      segments: [
        {
          originCrs: 'EUS',
          destinationCrs: 'PRE',
          cappedByMaxChanges: false,
          itineraries: [],
          noResultReason: {
            constraint: 'via',
            values: ['STA'],
            message:
              'No itinerary from EUS to PRE departing after 09:45 on 2026-10-08 passes through STA (calling there or not); one exists without that via.',
          },
        },
      ],
      journeys: [],
    };
    await search(vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(noVia) }));
    expect(await screen.findByText(/passes through STA \(calling there or not\)/)).toBeInTheDocument();
    expect(await screen.findByText(/Try removing STA · Stafford from "Pass through"/)).toBeInTheDocument();
  });

  it('suggests allowing more changes for a maxChanges no-result', async () => {
    const capped: TripPlanResponse = {
      results: 'options',
      segments: [
        {
          originCrs: 'EUS',
          destinationCrs: 'PRE',
          cappedByMaxChanges: true,
          itineraries: [],
          noResultReason: {
            constraint: 'maxChanges',
            values: [],
            message: 'Every itinerary needs more than 0 changes.',
          },
        },
      ],
    };
    await search(vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(capped) }));
    expect(await screen.findByText(/Allow more changes in Advanced options/)).toBeInTheDocument();
  });
});

describe('PlanTripFlow arrive-by and paging (2026-10-19)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    mockStationNames.clear();
    window.history.replaceState(null, '', '/');
  });

  // A future date, so the form keeps it (a past one is replaced by today).
  const DATE = '2030-10-24';

  function planWith(departure: [string, number], arrival: [string, number]): TripPlanResponse {
    return {
      ...singleSegmentPlan,
      journeys: [
        {
          changeCount: 0,
          departure: { time: departure[0], dayOffset: departure[1] },
          arrival: { time: arrival[0], dayOffset: arrival[1] },
          totalDurationMinutes: 50,
        },
      ],
    };
  }

  function ok(plan: TripPlanResponse) {
    return { ok: true, json: () => Promise.resolve(plan) };
  }

  function sentParams(fetchMock: ReturnType<typeof vi.fn>, call: number): URLSearchParams {
    return new URL(String(fetchMock.mock.calls[call]![0]), 'http://localhost').searchParams;
  }

  async function searchArriveBy(fetchMock: ReturnType<typeof vi.fn>, arriveBy: string) {
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <PlanTripFlow
        onCreated={vi.fn()}
        syncUrl
        initialQuery={{ originCrs: 'EUS', destinationCrs: 'MKC', date: DATE, arriveBy }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
  }

  it('plans by arrival time and says so above the results', async () => {
    window.history.replaceState(null, '', '/plan');
    const fetchMock = vi.fn().mockResolvedValueOnce(ok(planWith(['08:00:00', 0], ['08:50:00', 0])));
    await searchArriveBy(fetchMock, '09:00');
    const sent = sentParams(fetchMock, 0);
    expect(sent.get('arriveBy')).toBe('09:00:00');
    expect(sent.has('departAfter')).toBe(false);
    expect(sent.has('dayOffset')).toBe(false);
    expect(await screen.findByText('Arrive by 09:00, 24 Oct 2030')).toBeInTheDocument();
    expect(window.location.search).toContain('arriveBy=09%3A00');
    expect(window.location.search).not.toContain('departAfter');
  });

  it("sends an arrival just after midnight as the previous day across midnight, but shows the visitor's date", async () => {
    window.history.replaceState(null, '', '/plan');
    const fetchMock = vi.fn().mockResolvedValueOnce(ok(planWith(['23:30:00', 0], ['00:20:00', 1])));
    await searchArriveBy(fetchMock, '00:30');
    const sent = sentParams(fetchMock, 0);
    expect(sent.get('date')).toBe('2030-10-23');
    expect(sent.get('arriveBy')).toBe('00:30:00');
    expect(sent.get('dayOffset')).toBe('1');
    expect(sent.get('acrossMidnight')).toBe('true');
    expect(await screen.findByText('Arrive by 00:30, 24 Oct 2030')).toBeInTheDocument();
    // The address bar keeps what the visitor asked for.
    expect(window.location.search).toContain(`date=${DATE}`);
    expect(window.location.search).not.toContain('dayOffset');
  });

  /** A page of one-train journeys (uid, departure, arrival, live minutes late). */
  function pageOf(...rides: [string, string, string, number?][]): TripPlanResponse {
    const at = (time: string, late = 0) => {
      const [h = 0, m = 0] = time.split(':').map(Number);
      const total = h * 60 + m + late;
      const day = Math.floor(total / 1440);
      const rest = total - day * 1440;
      return {
        time: `${String(Math.floor(rest / 60)).padStart(2, '0')}:${String(rest % 60).padStart(2, '0')}:00`,
        dayOffset: day,
      };
    };
    return {
      results: 'fastest',
      segments: [
        {
          originCrs: 'EUS',
          destinationCrs: 'MKC',
          cappedByMaxChanges: false,
          itineraries: rides.map(([uid, departure, arrival]) => ({
            legs: [
              {
                kind: 'train' as const,
                trainUid: uid,
                serviceDate: DATE,
                originCrs: 'EUS',
                destinationCrs: 'MKC',
                scheduledDeparture: `${departure}:00`,
                scheduledArrival: `${arrival}:00`,
                arrivalDayOffset: 0,
              },
            ],
            changeCount: 0,
            totalDurationMinutes: 50,
          })),
        },
      ],
      journeys: rides.map(([, departure, arrival, late]) => ({
        changeCount: 0,
        departure: at(departure, late),
        arrival: at(arrival, late),
        totalDurationMinutes: 50,
      })),
    };
  }

  /** The cards' trains, top to bottom. */
  function shownTrains(): string[] {
    return Array.from(document.querySelectorAll('[data-plan-entry]')).map(
      (card) => /^0:(\w+)@/.exec(card.getAttribute('data-plan-entry') ?? '')?.[1] ?? '?',
    );
  }

  it('appends later routes below and prepends earlier ones above, keeping the search, form and URL', async () => {
    window.history.replaceState(null, '', '/plan');
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(pageOf(['B', '08:00', '08:50'])))
      .mockResolvedValueOnce(ok(pageOf(['C', '08:30', '09:20'])))
      .mockResolvedValueOnce(ok(pageOf(['A', '07:30', '08:20'])));
    await searchArriveBy(fetchMock, '09:00');
    await waitFor(() => expect(shownTrains()).toEqual(['B']));

    fireEvent.click(screen.getByRole('button', { name: 'Later routes' }));
    await waitFor(() => expect(shownTrains()).toEqual(['B', 'C']));
    let sent = sentParams(fetchMock, 1);
    // Just after the latest departure shown.
    expect(sent.get('departAfter')).toBe('08:01:00');
    expect(sent.has('arriveBy')).toBe(false);
    expect(sent.get('date')).toBe(DATE);

    fireEvent.click(screen.getByRole('button', { name: 'Earlier routes' }));
    await waitFor(() => expect(shownTrains()).toEqual(['A', 'B', 'C']));
    sent = sentParams(fetchMock, 2);
    // Just before the earliest arrival shown.
    expect(sent.get('arriveBy')).toBe('08:49:00');
    expect(sent.has('departAfter')).toBe(false);

    // The visitor's own search stays: summary, form and address bar.
    expect(screen.getByTestId('plan-timing-summary')).toHaveTextContent('Arrive by 09:00, 24 Oct 2030');
    expect(screen.getByRole('radio', { name: 'Arrive by' })).toBeChecked();
    expect(screen.getByLabelText('Time')).toHaveValue('09:00');
    expect(window.location.search).toContain('arriveBy=09%3A00');
    expect(window.location.search).not.toContain('departAfter');
  });

  it('pages after a late first train from its live departure and never shows it twice', async () => {
    // B is booked 08:00 and running 10 late, so the planner had it at 08:10.
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(pageOf(['B', '08:00', '08:50', 10])))
      // Its delay grew: the same train, now after the cursor. Nothing new,
      // so the click asks again from the new latest departure.
      .mockResolvedValueOnce(ok(pageOf(['B', '08:00', '08:50', 13])))
      .mockResolvedValueOnce(ok(pageOf(['C', '08:30', '09:20'])));
    await searchArriveBy(fetchMock, '09:00');
    await waitFor(() => expect(shownTrains()).toEqual(['B']));
    fireEvent.click(screen.getByRole('button', { name: 'Later routes' }));
    await waitFor(() => expect(shownTrains()).toEqual(['B', 'C']));
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(sentParams(fetchMock, 1).get('departAfter')).toBe('08:11:00');
    expect(sentParams(fetchMock, 2).get('departAfter')).toBe('08:14:00');
  });

  it('says when there is nothing earlier on this day, and keeps what is shown', async () => {
    const empty: TripPlanResponse = {
      results: 'fastest',
      segments: [
        {
          ...singleSegmentPlan.segments[0]!,
          itineraries: [],
          noResultReason: { constraint: 'arriveBy', values: ['07:29'], message: 'Nothing arrives by 07:29.' },
        },
      ],
      journeys: [],
    };
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(pageOf(['B', '08:00', '08:50'])))
      .mockResolvedValueOnce(ok(empty));
    await searchArriveBy(fetchMock, '09:00');
    fireEvent.click(await screen.findByRole('button', { name: 'Earlier routes' }));
    expect(await screen.findByText('No earlier journeys on this day.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Earlier routes' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Later routes' })).toBeEnabled();
    expect(shownTrains()).toEqual(['B']);
    expect(screen.queryByText(/No route found/)).not.toBeInTheDocument();
  });

  it('keeps the chosen route chosen while more are loaded', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(pageOf(['B', '08:00', '08:50'])))
      .mockResolvedValueOnce(ok(pageOf(['A', '07:30', '08:20'])));
    await searchArriveBy(fetchMock, '09:00');
    await waitFor(() => expect(shownTrains()).toEqual(['B']));
    const options = () => screen.getAllByRole('radio').filter((radio) => radio.closest('[data-plan-entry]'));
    fireEvent.click(options()[0]!);
    expect(screen.getByRole('button', { name: 'Track this journey' })).toBeEnabled();
    fireEvent.click(screen.getByRole('button', { name: 'Earlier routes' }));
    await waitFor(() => expect(shownTrains()).toEqual(['A', 'B']));
    expect(options()[1]).toBeChecked();
    expect(screen.getByRole('button', { name: 'Track this journey' })).toBeEnabled();
  });

  it('holds the card on screen still when earlier routes go in above it', async () => {
    const scrollBy = vi.spyOn(window, 'scrollBy').mockImplementation(() => {});
    // Each card 100px tall, the first one half scrolled past.
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      const cards = Array.from(document.querySelectorAll('[data-plan-entry]'));
      const top = cards.indexOf(this) * 100 - 50;
      return { top, bottom: top + 100, left: 0, right: 0, width: 0, height: 100, x: 0, y: top } as DOMRect;
    });
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(pageOf(['C', '08:00', '08:50'])))
      .mockResolvedValueOnce(ok(pageOf(['A', '07:00', '07:50'], ['B', '07:30', '08:20'])));
    await searchArriveBy(fetchMock, '09:00');
    await waitFor(() => expect(shownTrains()).toEqual(['C']));
    fireEvent.click(screen.getByRole('button', { name: 'Earlier routes' }));
    await waitFor(() => expect(shownTrains()).toEqual(['A', 'B', 'C']));
    // C moved down two cards; the window follows it.
    expect(scrollBy).toHaveBeenCalledWith(0, 200);
  });

  it('shows a failed page by its button and keeps the results', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(pageOf(['B', '08:00', '08:50'])))
      .mockRejectedValueOnce(new TypeError('Failed to fetch'));
    await searchArriveBy(fetchMock, '09:00');
    fireEvent.click(await screen.findByRole('button', { name: 'Later routes' }));
    expect(await screen.findByText(/Couldn't load later routes\./)).toBeInTheDocument();
    expect(shownTrains()).toEqual(['B']);
    expect(screen.getByRole('button', { name: 'Later routes' })).toBeEnabled();
  });

  it('starts afresh on a new search', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(pageOf(['B', '08:00', '08:50'])))
      .mockResolvedValueOnce(ok(pageOf(['C', '08:30', '09:20'])))
      .mockResolvedValueOnce(ok(pageOf(['D', '08:10', '09:00'])));
    await searchArriveBy(fetchMock, '09:00');
    fireEvent.click(await screen.findByRole('button', { name: 'Later routes' }));
    await waitFor(() => expect(shownTrains()).toEqual(['B', 'C']));
    fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
    await waitFor(() => expect(shownTrains()).toEqual(['D']));
  });

  it('offers no paging before a search', () => {
    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    expect(screen.queryByRole('button', { name: 'Later routes' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Earlier routes' })).not.toBeInTheDocument();
  });
});

describe('PlanTripFlow with tram legs', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    mockStationNames.clear();
  });

  const tramPlan: TripPlanResponse = {
    results: 'fastest',
    segments: [{ originCrs: 'SPT', destinationCrs: 'LDS', cappedByMaxChanges: false, itineraries: [RAIL_TRAM_RAIL] }],
    transitNotices: [],
  };

  async function search() {
    renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'SPT' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'LDS' } });
    fireEvent.click(screen.getByText('Find routes'));
  }

  it('asks for tram legs, and shows the tram and its caption', async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(tramPlan) });
    vi.stubGlobal('fetch', fetchMock);
    await search();
    await screen.findByText(byVisibleText('08:14 Tram (Blue Line) Piccadilly to Victoria 08:22'));
    expect(screen.getByText('Scheduled times, not live')).toBeInTheDocument();
    const url = String(fetchMock.mock.calls[0]![0]);
    expect(new URLSearchParams(url.slice(url.indexOf('?') + 1)).get('transit')).toBe('true');
  });

  it("shows a network's notice as information, in the api's words", async () => {
    const plan: TripPlanResponse = {
      ...tramPlan,
      segments: [{ ...tramPlan.segments[0]!, itineraries: [] }],
      transitNotices: [
        { network: 'metrolink', reason: 'noTimetable', message: "Tram times aren't available for this date." },
      ],
    };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(plan) }));
    const { container } = renderWithMantine(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'SPT' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'LDS' } });
    fireEvent.click(screen.getByText('Find routes'));
    await screen.findByText("Tram times aren't available for this date.");
    expect(container.querySelector('[data-transit-notice]')).not.toBeNull();
    expect(container.textContent).not.toMatch(/no trams/i);
  });

  it('saves the trains and the tram ride between them', async () => {
    mockStationNames.set('MAN', 'Manchester Piccadilly');
    mockStationNames.set('MCV', 'Manchester Victoria');
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(tramPlan) })
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
      });
    vi.stubGlobal('fetch', fetchMock);
    const onCreated = vi.fn();
    renderWithMantine(<PlanTripFlow onCreated={onCreated} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'SPT' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'LDS' } });
    fireEvent.click(screen.getByText('Find routes'));
    await screen.findByText(byVisibleText('08:14 Tram (Blue Line) Piccadilly to Victoria 08:22'));
    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]!);
    fireEvent.click(screen.getByText('Track this journey'));
    await waitFor(() => expect(onCreated).toHaveBeenCalled());
    const body = JSON.parse(String((fetchMock.mock.calls[1]![1] as RequestInit).body)) as {
      legs: { kind?: string; transitMode?: string; originStopId?: string }[];
    };
    expect(body.legs.map((leg) => leg.transitMode ?? 'train')).toEqual(['train', 'walk', 'tram', 'walk', 'train']);
    expect(body.legs[2]).toMatchObject({ kind: 'untracked', originStopId: '9400ZZMAPIC' });
  });
});

describe('PlanTripFlow operator filter (2026-10-31)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    mockStationNames.clear();
    window.history.replaceState(null, '', '/');
  });

  const operatorCatalogue: PlannerOperatorCatalogue = {
    operators: [
      { code: 'LT', name: 'London Underground' },
      { code: 'SN', name: 'Southern' },
      { code: 'VT', name: 'Avanti West Coast' },
    ],
    groups: [],
    dsOperators: [],
  };

  const filteredPlan: TripPlanResponse = {
    ...singleSegmentPlan,
    operator: ['VT'],
    avoidOperator: [],
    segments: [
      {
        ...singleSegmentPlan.segments[0]!,
        itineraries: [
          {
            ...singleSegmentPlan.segments[0]!.itineraries[0]!,
            legs: [{ ...(singleSegmentPlan.segments[0]!.itineraries[0]!.legs[0] as object), operator: 'VT' } as never],
          },
        ],
      },
    ],
  };

  const noTubePlan: TripPlanResponse = {
    results: 'fastest',
    operator: ['SN', 'VT'],
    avoidOperator: [],
    segments: [
      {
        originCrs: 'MAN',
        destinationCrs: 'BTN',
        itineraries: [],
        cappedByMaxChanges: false,
        noResultReason: {
          constraint: 'operator',
          values: ['SN', 'VT'],
          linkOperators: ['LT'],
          message:
            'No itinerary from MAN to BTN while using only SN, VT: the route needs LT, which this filter leaves out.',
        },
      },
    ],
  };

  async function searchWith(fetchMock: ReturnType<typeof vi.fn>, initial: PlanFormInitial) {
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <PlanTripFlow onCreated={vi.fn()} operatorCatalogue={operatorCatalogue} initialQuery={initial} />,
    );
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'MAN' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'BTN' } });
    fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
  }

  it('says which filter the results used, and names each leg operator', async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(filteredPlan) });
    await searchWith(fetchMock, { operatorMode: 'only', operators: ['VT'] });
    expect(
      await screen.findByText(
        /Showing routes using only Avanti West Coast\. Walks between stations are included; the Tube is not\./,
      ),
    ).toBeInTheDocument();
    expect(String(fetchMock.mock.calls[0]![0])).toContain('operator=VT');
    expect(screen.getByText('Avanti West Coast', { selector: '[data-leg-operator]' })).toBeInTheDocument();
    // "Change" opens Advanced options.
    fireEvent.click(screen.getByRole('button', { name: 'Change' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Advanced options/ })).toHaveAttribute('aria-expanded', 'true'),
    );
  });

  it('adds London Underground and searches again when the route needs the Tube', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(noTubePlan) })
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(filteredPlan) });
    await searchWith(fetchMock, { operatorMode: 'only', operators: ['VT', 'SN'] });
    expect(await screen.findByText(/Crossing London needs the Tube/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Add London Underground' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
    expect(new URL(String(fetchMock.mock.calls[1]![0]), 'http://x').searchParams.get('operator')).toBe('VT,SN,LT');
    // The form now holds it too.
    await waitFor(() =>
      expect(screen.getByText(/Only Avanti West Coast, Southern, London Underground/)).toBeInTheDocument(),
    );
  });
});

describe('PlanTripFlow Prefer (2026-10-31)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    mockStationNames.clear();
    window.history.replaceState(null, '', '/');
  });

  const catalogue: PlannerOperatorCatalogue = {
    operators: [{ code: 'GR', name: 'LNER' }],
    groups: [],
    dsOperators: [],
  };

  async function searchPreferring(plan: TripPlanResponse) {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(plan) }));
    renderWithMantine(
      <PlanTripFlow
        onCreated={vi.fn()}
        operatorCatalogue={catalogue}
        initialQuery={{ results: 'options', operatorMode: 'prefer', operators: ['GR'] }}
      />,
    );
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
  }

  it('labels the operator-only option by name', async () => {
    const itinerary = singleSegmentPlan.segments[0]!.itineraries[0]!;
    await searchPreferring({
      ...singleSegmentPlan,
      results: 'options',
      preferOperator: ['GR'],
      preferOperatorResult: 'added',
      segments: [
        {
          ...singleSegmentPlan.segments[0]!,
          itineraries: [{ ...itinerary, labels: [{ kind: 'operatorOnly', operators: ['GR'] }] }],
        },
      ],
    });
    expect(await screen.findByText('LNER only')).toBeInTheDocument();
    expect(screen.queryByText(/Showing routes/)).not.toBeInTheDocument();
  });

  it('says when the operator-only route was too slow to show', async () => {
    await searchPreferring({
      ...singleSegmentPlan,
      results: 'options',
      preferOperator: ['GR'],
      preferOperatorResult: 'tooSlow',
    });
    expect(
      await screen.findByText("The best route using only LNER is much slower than these, so it isn't shown."),
    ).toBeInTheDocument();
  });
});
