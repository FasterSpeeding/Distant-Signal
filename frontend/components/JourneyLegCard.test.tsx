import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyLegCard } from './JourneyLegCard';
import type { JourneyLegDetail, TrackedTrainState } from '@/lib/types';

const pushMock = vi.fn();
const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, refresh: refreshMock }),
  usePathname: () => '/journeys/167',
  useSearchParams: () => new URLSearchParams(''),
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
        scheduledDeparture: '2026-09-22T16:00:00Z',
        actualArrival: null,
        actualDeparture: '2026-09-22T16:00:00Z',
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
        scheduledArrival: '2026-09-22T18:00:00Z',
        scheduledDeparture: null,
        actualArrival: null,
        actualDeparture: null,
        estimatedArrival: '2026-09-22T18:22:00Z',
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
      {
        crs: 'EDB',
        name: 'Edinburgh',
        tiploc: null,
        kind: 'Terminate',
        scheduledArrival: '2026-09-22T20:00:00Z',
        scheduledDeparture: null,
        actualArrival: null,
        actualDeparture: null,
        estimatedArrival: '2026-09-22T20:22:00Z',
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

function baseLeg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
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
    // `false` by default -- matches a `pin`/`knownTrain`-mode leg with no
    // window at all. Individual tests below pass `windowSearched: true`
    // alongside a window-bound override wherever the fixture is meant to
    // represent a real window search (see `JourneyLegDetail.windowSearched`'s
    // own doc comment: this is what `hasWindow` reads now, not the raw
    // bounds).
    windowSearched: false,
    trackedTrainState: trackedState(),
    legSkip: null,
    ...overrides,
  };
}

describe('JourneyLegCard', () => {
  beforeEach(() => {
    // `JourneyLegCandidates` fetches its own candidate list on mount
    // whenever it's rendered (the open-leg branch always renders it for
    // an owner; the matched branch renders it once "Change train" is
    // clicked) -- default every test to a real resolved response so that
    // effect never throws calling `.then()` on `undefined`. Individual
    // tests can still override this with `vi.mocked(fetch).mockResolvedValue(...)`.
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(new Response(JSON.stringify({ results: [], nextCursor: null }), { status: 200 })),
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  // 2026-09-22 UX review finding I16/2.7: the leg's own route+time, not
  // the train's headcode, is the card's title.
  it('titles a matched leg by its own route and departure time, not the train headcode', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner isOnlyLeg={false} />);
    expect(screen.getByText('London Kings Cross (KGX) → York (YRK) · 17:00')).toBeInTheDocument();
  });

  it('still shows the headcode, as secondary information', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner isOnlyLeg={false} />);
    expect(screen.getByText('Train P9E010')).toBeInTheDocument();
  });

  // Feature request: journey-view legs should link through to their own
  // train tracking page. `/train/[uid]/[date]` is the public per-train
  // route (`app/train/[uid]/[date]/page.tsx`); the link uses the LEG's own
  // `serviceDate`, same source of truth `JourneyLegCandidates` already
  // reads off this card (`serviceDate={leg.serviceDate}`), not the matched
  // train's own `state.serviceDate`.
  it('links the headcode through to that train\'s own tracking page', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner isOnlyLeg={false} />);
    expect(screen.getByRole('link', { name: 'Train P9E010' })).toHaveAttribute(
      'href',
      '/train/P9E010/2026-09-22',
    );
  });

  // A leg can be "matched" (`trackedTrainState !== null`) while still
  // waiting for Network Rail's first live report to actually name the
  // service (`resolutionStatus: 'pending'`, `trainUid: null`) -- there is
  // no `/train/[uid]/...` page to link to yet, so no link (and no
  // headcode text at all) should render.
  it('renders no train link (or headcode) for a matched-but-unresolved leg', () => {
    renderWithMantine(
      <JourneyLegCard
        journeyId={167}
        leg={baseLeg({ trackedTrainState: trackedState({ resolutionStatus: 'pending', trainUid: null }) })}
        isOwner
        isOnlyLeg={false}
      />,
    );
    expect(screen.queryByRole('link', { name: /Train/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/^Train /)).not.toBeInTheDocument();
  });

  // 2026-09-22 UX review finding I14/2.4, later corrected: "Change train"
  // and "Remove leg" are INDEPENDENT actions, not a ternary. A matched,
  // windowed leg must get BOTH -- before this fix it only ever got
  // "Change train", leaving most legs on most journeys (anything created
  // via the time-window search flow) with no way to be removed outright.
  it('shows both "Change train" AND "Remove leg" for a matched, windowed, owner-viewed leg', () => {
    renderWithMantine(
      <JourneyLegCard
        journeyId={167}
        leg={baseLeg({ departAfter: '18:00:00', windowSearched: true })}
        isOwner
        isOnlyLeg={false}
      />,
    );
    expect(screen.getByRole('button', { name: 'Change train' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Remove leg' })).toBeInTheDocument();
  });

  // Regression test for the 19-pass security/bug review's journeys-area
  // Medium finding 1: a template leg with ALL FOUR window bounds
  // deliberately left unset (a fully-open "any train, any time" search --
  // `journey_templates::validate_template_leg`'s own doc comment) is
  // byte-for-byte the same bounds shape as a `pin`/`knownTrain`-mode leg
  // that never had a window at all. Before this fix, `hasWindow` was
  // derived purely from those four bounds, so once such a leg was first
  // matched, "Change train" vanished permanently. `windowSearched` now
  // carries that distinction independently of the bounds themselves.
  it('shows "Change train" for a matched leg with an intentionally fully-open window (no bounds, windowSearched true)', () => {
    renderWithMantine(
      <JourneyLegCard journeyId={167} leg={baseLeg({ windowSearched: true })} isOwner isOnlyLeg={false} />,
    );
    expect(screen.getByRole('button', { name: 'Change train' })).toBeInTheDocument();
  });

  // A no-window leg (a direct pin/known-train pick) has nothing to
  // re-search, so it gets "Remove leg" only -- never "Change train".
  it('shows "Remove leg" but not "Change train" for a matched, no-window, owner-viewed leg', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner isOnlyLeg={false} />);
    expect(screen.getByRole('button', { name: 'Remove leg' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Change train' })).not.toBeInTheDocument();
  });

  it('offers neither action to a non-owner (shared-group viewer)', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner={false} isOnlyLeg={false} />);
    expect(screen.queryByRole('button', { name: 'Remove leg' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Change train' })).not.toBeInTheDocument();
  });

  it('removing the leg calls DELETE and refreshes when a sibling leg remains', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg({ id: 5 })} isOwner isOnlyLeg={false} />);

    fireEvent.click(screen.getByRole('button', { name: 'Remove leg' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove leg' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove leg' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/167/legs/5', { method: 'DELETE' }));
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('clicking "Change train" toggles the candidate picker open', () => {
    renderWithMantine(
      <JourneyLegCard
        journeyId={167}
        leg={baseLeg({ departAfter: '18:00:00', windowSearched: true })}
        isOwner
        isOnlyLeg={false}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Change train' }));
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
  });

  // The traveller's own alighting station, distinct from the underlying
  // train's terminus.
  it('marks the leg destination row "You get off here" in the timeline', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner isOnlyLeg={false} />);
    expect(screen.getByText('You get off here')).toBeInTheDocument();
  });

  it('titles an open leg by its own route, in the same date format the matched card uses', () => {
    renderWithMantine(
      <JourneyLegCard
        journeyId={167}
        leg={baseLeg({ trackedTrainState: null, originCrs: 'YRK', destinationCrs: 'NCL' })}
        isOwner
        isOnlyLeg={false}
      />,
    );
    // 2026-09-22 UX review finding 2.9: "YRK → NCL, 2026-09-22" (a raw ISO
    // date) is gone -- the open leg now uses the same `formatDate` the
    // matched card's own title does ("22 Sept 2026", not "2026-09-22").
    expect(screen.getByText('YRK → NCL, 22 Sept 2026')).toBeInTheDocument();
    expect(screen.queryByText(/2026-09-22/)).not.toBeInTheDocument();
    // The old unconditional "Searching…" copy is gone: `JourneyLegCandidates`
    // now owns its own state text (its match-count line).
    expect(screen.queryByText('Searching for a train to track — pick one below.')).not.toBeInTheDocument();
  });

  // 2026-09-22 UX review finding I18: "the window the user just typed is
  // never shown back to them" -- an open leg's own departAfter/etc. used
  // to be silently dropped from the card entirely.
  it('shows the leg\'s own search window on an open leg', () => {
    renderWithMantine(
      <JourneyLegCard
        journeyId={167}
        leg={baseLeg({
          trackedTrainState: null,
          originCrs: 'YRK',
          destinationCrs: 'NCL',
          departAfter: '18:00:00',
          arriveBefore: '21:30:00',
        })}
        isOwner
        isOnlyLeg={false}
      />,
    );
    expect(
      screen.getByText('Departing at or after 18:00 · arriving at or before 21:30'),
    ).toBeInTheDocument();
  });

  // 2026-09-22 UX review addendum: with both bounds on the SAME side set
  // (e.g. depart-after + depart-before, no arrive bound at all), the naive
  // per-field join used to repeat the verb back to back -- "Departing at
  // or after 19:00 · departing at or before 21:00" -- which reads like a
  // copy-paste bug. Each of the seven realistic depart/arrive bound
  // combinations is exercised here (the pre-existing test above already
  // covers the eighth: a depart+arrive mix).
  it.each([
    {
      name: 'only depart-after',
      overrides: { departAfter: '19:00:00' },
      expected: 'Departing at or after 19:00',
    },
    {
      name: 'only depart-before',
      overrides: { departBefore: '21:00:00' },
      expected: 'Departing at or before 21:00',
    },
    {
      name: 'both depart bounds, no arrive bound',
      overrides: { departAfter: '19:00:00', departBefore: '21:00:00' },
      expected: 'Departing between 19:00 and 21:00',
    },
    {
      name: 'only arrive-after',
      overrides: { arriveAfter: '19:00:00' },
      expected: 'Arriving at or after 19:00',
    },
    {
      name: 'only arrive-before',
      overrides: { arriveBefore: '21:00:00' },
      expected: 'Arriving at or before 21:00',
    },
    {
      name: 'both arrive bounds, no depart bound',
      overrides: { arriveAfter: '19:00:00', arriveBefore: '21:00:00' },
      expected: 'Arriving between 19:00 and 21:00',
    },
    {
      name: 'a depart+arrive mix with both bounds set on each side',
      overrides: {
        departAfter: '18:00:00',
        departBefore: '19:00:00',
        arriveAfter: '20:00:00',
        arriveBefore: '21:00:00',
      },
      expected: 'Departing between 18:00 and 19:00 · arriving between 20:00 and 21:00',
    },
  ])('shows the search window for $name', ({ overrides, expected }) => {
    renderWithMantine(
      <JourneyLegCard
        journeyId={167}
        leg={baseLeg({
          trackedTrainState: null,
          originCrs: 'YRK',
          destinationCrs: 'NCL',
          departAfter: null,
          departBefore: null,
          arriveAfter: null,
          arriveBefore: null,
          ...overrides,
        })}
        isOwner
        isOnlyLeg={false}
      />,
    );
    expect(screen.getByText(expected)).toBeInTheDocument();
  });

  it('shows nothing extra for an open leg with no window at all', () => {
    renderWithMantine(
      <JourneyLegCard
        journeyId={167}
        leg={baseLeg({ trackedTrainState: null, originCrs: 'YRK', destinationCrs: 'NCL' })}
        isOwner
        isOnlyLeg={false}
      />,
    );
    expect(screen.queryByText(/Departing|Arriving/)).not.toBeInTheDocument();
  });
});

/** An OPEN (unmatched) leg, on top of the matched `baseLeg` fixture above
 * -- the two branches of this component are different enough that the
 * open-leg tests below read better from their own starting point. */
function openLeg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return baseLeg({
    destinationCrs: 'EDB',
    matchMode: 'unmatched',
    trackedTrainState: null,
    ...overrides,
  });
}

/** Every owner-visible open-leg card mounts `JourneyLegCandidates`, which
 * fires a real fetch on mount -- awaiting its zero-result state lets that
 * effect settle inside `act()` before a test's own assertions run. */
async function settleCandidates() {
  await screen.findByText(/No scheduled trains match this window\./);
}

describe('JourneyLegCard (open leg)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders resolved station names in the header, not raw CRS codes, when names resolve', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(
      <JourneyLegCard
        journeyId={1}
        isOwner
        isOnlyLeg={false}
        leg={openLeg({ originName: 'London Kings Cross', destinationName: 'Edinburgh' })}
      />,
    );
    await settleCandidates();

    expect(screen.getByText('London Kings Cross (KGX) → Edinburgh (EDB), 22 Sept 2026')).toBeInTheDocument();
    expect(screen.queryByText(/KGX → EDB/)).not.toBeInTheDocument();
  });

  it('falls back to bare CRS codes when no station name resolved, still using the shared formatter', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner isOnlyLeg={false} leg={openLeg()} />);
    await settleCandidates();

    expect(screen.getByText('KGX → EDB, 22 Sept 2026')).toBeInTheDocument();
  });

  it('shows the persisted search window back to the user, and an Edit search link for the owner', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(
      <JourneyLegCard
        journeyId={1}
        isOwner
        isOnlyLeg={false}
        leg={openLeg({ departAfter: '08:00:00', arriveBefore: '12:30:00' })}
      />,
    );
    await settleCandidates();

    expect(
      screen.getByText('Departing at or after 08:00 · arriving at or before 12:30'),
    ).toBeInTheDocument();
    const editLink = screen.getByRole('link', { name: 'Edit search' });
    expect(editLink).toHaveAttribute('href', '/track?mode=window&origin=KGX');
  });

  it('does not offer Edit search to a non-owning group member', () => {
    // A non-owner never renders `JourneyLegCandidates` at all (the API
    // would 404 a pick attempt anyway) -- no fetch to await here.
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner={false} isOnlyLeg={false} leg={openLeg({ departAfter: '08:00:00' })} />);

    expect(screen.queryByRole('link', { name: 'Edit search' })).not.toBeInTheDocument();
    // Still shows the criteria, just without the edit affordance.
    expect(screen.getByText(/Departing at or after 08:00/)).toBeInTheDocument();
  });

  it('renders no window-criteria line when the leg has no persisted bound', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner isOnlyLeg={false} leg={openLeg()} />);
    await settleCandidates();

    expect(screen.queryByText(/Departing at or|Arriving at or/)).not.toBeInTheDocument();
  });

  it('no longer shows the old unconditional "Searching…" copy — the candidate list owns its own state text', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner isOnlyLeg={false} leg={openLeg()} />);
    await settleCandidates();

    expect(screen.queryByText('Searching for a train to track — pick one below.')).not.toBeInTheDocument();
  });

  // An open leg has no `trackedTrainState` at all yet -- there is no
  // `/train/[uid]/[date]` page to link to until a train is picked, so this
  // card's own `JourneyLegCandidates` picker offers "View live status"
  // links per candidate row instead (see that component's own test file);
  // the card itself renders no top-level train link.
  it('renders no top-level train link on an open leg', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner isOnlyLeg={false} leg={openLeg()} />);
    await settleCandidates();

    expect(screen.queryByRole('link', { name: /^Train / })).not.toBeInTheDocument();
  });

  // The other half of the ternary-gap fix: an open (never-yet-matched)
  // leg previously had NO delete affordance at all -- only the matched
  // branch ever rendered `RemoveJourneyLegButton`. The backend route
  // never cared whether the leg was matched, so this was a frontend-only
  // gap.
  it('offers "Remove leg" to the owner on an open leg too', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner isOnlyLeg={false} leg={openLeg()} />);
    await settleCandidates();

    expect(screen.getByRole('button', { name: 'Remove leg' })).toBeInTheDocument();
  });

  it('does not offer "Remove leg" to a non-owning group member on an open leg', () => {
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner={false} isOnlyLeg={false} leg={openLeg()} />);

    expect(screen.queryByRole('button', { name: 'Remove leg' })).not.toBeInTheDocument();
  });

  it('removing an open leg calls DELETE, same as a matched leg', async () => {
    const fetchMock = vi.fn();
    fetchMock
      .mockResolvedValueOnce(new Response('{"results":[],"nextCursor":null}', { status: 200 }))
      .mockResolvedValueOnce(new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner isOnlyLeg={false} leg={openLeg({ id: 9 })} />);
    await settleCandidates();

    fireEvent.click(screen.getByRole('button', { name: 'Remove leg' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove leg' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove leg' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/1/legs/9', { method: 'DELETE' }));
  });

  // Review §2.3/I15: the open-leg card is the one that needs the user to
  // do something -- it used to be visually indistinguishable from a plain
  // white box. A left accent border now flags it, on both the owner and
  // non-owner branches (only the OWNER can act, but both are states that
  // need a train picked).
  it('gives the open-leg card a left accent border, unlike a plain card', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    const { container } = renderWithMantine(<JourneyLegCard journeyId={1} isOwner isOnlyLeg={false} leg={openLeg()} />);
    await settleCandidates();

    const card = container.querySelector('.mantine-Card-root');
    expect(card).toHaveStyle({ borderLeftWidth: '4px' });
  });

  it('accents the non-owner "waiting for the owner" card the same way', () => {
    const { container } = renderWithMantine(<JourneyLegCard journeyId={1} isOwner={false} isOnlyLeg={false} leg={openLeg()} />);

    const card = container.querySelector('.mantine-Card-root');
    expect(card).toHaveStyle({ borderLeftWidth: '4px' });
    expect(screen.getByText('Waiting for the owner to pick a train.')).toBeInTheDocument();
  });
});
