import { screen, fireEvent } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { ItineraryOption } from './ItineraryOption';
import type { TripPlanItinerary } from '@/lib/types';
import { byVisibleText } from '@/test/routeText';

const trainItinerary: TripPlanItinerary = {
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
};

const fixedLinkOnlyItinerary: TripPlanItinerary = {
  legs: [{ kind: 'transfer', mode: 'TUBE', originCrs: 'EUS', destinationCrs: 'KGX', minutes: 5 }],
  changeCount: 0,
  totalDurationMinutes: 5,
};

describe('ItineraryOption', () => {
  it('renders a train leg summary and allows selection', () => {
    const onSelect = vi.fn();
    renderWithMantine(<ItineraryOption itinerary={trainItinerary} selected={false} onSelect={onSelect} />);
    expect(screen.getByText(byVisibleText(/08:00 EUS to MKC 08:50/))).toBeInTheDocument();
    fireEvent.click(screen.getByRole('radio'));
    expect(onSelect).toHaveBeenCalled();
  });

  it('flags an itinerary that exceeds the recommended change count', () => {
    renderWithMantine(
      <ItineraryOption
        itinerary={{ ...trainItinerary, changeCount: 3, exceedsRecommendedChanges: true }}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText('More changes than usually recommended')).toBeInTheDocument();
  });

  it('annotates a late or cancelled leg and an itinerary live data breaks', () => {
    const live = {
      status: 'Late' as const,
      cancelled: false,
      delayMinutes: 12,
      arrivalDelayMinutes: 9,
      reason: null,
      reasonSource: null,
      platform: null,
      observedAt: null,
      interchangeFeasible: null,
    };
    const leg = trainItinerary.legs[0]!;
    const { unmount } = renderWithMantine(
      <ItineraryOption
        itinerary={{ ...trainItinerary, legs: [{ ...leg, live } as typeof leg], liveFeasible: true }}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText(byVisibleText(/08:00 EUS to MKC 08:50 · 12 min late/))).toBeInTheDocument();
    expect(screen.queryByText('Live data says this route may no longer work')).not.toBeInTheDocument();
    unmount();

    const early = renderWithMantine(
      <ItineraryOption
        itinerary={{ ...trainItinerary, legs: [{ ...leg, live: { ...live, delayMinutes: -3 } } as typeof leg] }}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText(byVisibleText(/08:00 EUS to MKC 08:50 · 3 min early/))).toBeInTheDocument();
    early.unmount();

    renderWithMantine(
      <ItineraryOption
        itinerary={{
          ...trainItinerary,
          legs: [{ ...leg, live: { ...live, status: 'Cancelled', cancelled: true } } as typeof leg],
          liveFeasible: false,
        }}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText(/08:50 · Cancelled/)).toBeInTheDocument();
    expect(screen.getByText('Live data says this route may no longer work')).toBeInTheDocument();
  });

  it('disables selection for a fixed-link-only itinerary with no train leg', () => {
    renderWithMantine(<ItineraryOption itinerary={fixedLinkOnlyItinerary} selected={false} onSelect={vi.fn()} />);
    expect(screen.getByRole('radio')).toBeDisabled();
    expect(screen.getByText('This route needs no train, so there is nothing to track.')).toBeInTheDocument();
  });

  // The bug this fixes: `GET /Trips/plan` never sends a name alongside a
  // leg's bare `originCrs`/`destinationCrs` (unlike every other
  // station-bearing response in this app), so this line used to render
  // ONLY the raw codes. `PlanTripFlow` (the only real caller) resolves
  // names itself and passes them down via `stationNames`; this proves the
  // leg summary renders `CODE · Name` once a name is available, for both
  // a train leg and a transfer leg.
  it('renders "CODE · Name" for each leg end once stationNames resolves a name', () => {
    const stationNames = new Map([
      ['EUS', 'London Euston'],
      ['MKC', 'Milton Keynes Central'],
    ]);
    renderWithMantine(
      <ItineraryOption itinerary={trainItinerary} selected={false} onSelect={vi.fn()} stationNames={stationNames} />,
    );
    expect(
      screen.getByText(byVisibleText('08:00 EUS · London Euston to MKC · Milton Keynes Central 08:50')),
    ).toBeInTheDocument();
  });

  it('renders "CODE · Name" for a transfer leg once stationNames resolves a name', () => {
    const stationNames = new Map([
      ['EUS', 'London Euston'],
      ['KGX', 'London Kings Cross'],
    ]);
    renderWithMantine(
      <ItineraryOption
        itinerary={fixedLinkOnlyItinerary}
        selected={false}
        onSelect={vi.fn()}
        stationNames={stationNames}
      />,
    );
    expect(
      screen.getByText(byVisibleText('Tube EUS · London Euston to KGX · London Kings Cross, 5 min')),
    ).toBeInTheDocument();
  });

  it("names a transfer's line when the planner knows it", () => {
    const itinerary: TripPlanItinerary = {
      legs: [
        { kind: 'transfer', mode: 'TUBE', originCrs: 'VXH', destinationCrs: 'EUS', minutes: 22, line: 'Victoria' },
      ],
      changeCount: 0,
      totalDurationMinutes: 22,
    };
    renderWithMantine(<ItineraryOption itinerary={itinerary} selected={false} onSelect={vi.fn()} />);
    expect(screen.getByText(byVisibleText('Tube (Victoria line) VXH to EUS, 22 min'))).toBeInTheDocument();
  });

  it('falls back to bare codes for an end whose name did not resolve, without mixing forms', () => {
    // Only EUS resolved -- per `codeRouteLabel`'s own "never mix" rule
    // (mirroring `routeLabel`'s), BOTH ends must render as bare codes
    // rather than "EUS · London Euston to MKC".
    const stationNames = new Map([['EUS', 'London Euston']]);
    renderWithMantine(
      <ItineraryOption itinerary={trainItinerary} selected={false} onSelect={vi.fn()} stationNames={stationNames} />,
    );
    expect(screen.getByText(byVisibleText('08:00 EUS to MKC 08:50'))).toBeInTheDocument();
  });

  it('renders bare codes when stationNames is omitted (default empty map)', () => {
    renderWithMantine(<ItineraryOption itinerary={trainItinerary} selected={false} onSelect={vi.fn()} />);
    expect(screen.getByText(byVisibleText('08:00 EUS to MKC 08:50'))).toBeInTheDocument();
  });
});

describe('ItineraryOption: bus and ferry legs', () => {
  it('labels a bus and a ferry leg, and leaves a train leg unlabelled', () => {
    const [trainLeg] = trainItinerary.legs;
    if (trainLeg?.kind !== 'train') throw new Error('fixture');
    const itinerary: TripPlanItinerary = {
      legs: [
        { ...trainLeg, serviceMode: 'train', liveTracking: true },
        { ...trainLeg, trainUid: 'C30818', serviceMode: 'replacementBus', liveTracking: false },
        { ...trainLeg, trainUid: 'S00001', serviceMode: 'ferry', liveTracking: false },
      ],
      changeCount: 2,
      totalDurationMinutes: 50,
    };
    renderWithMantine(<ItineraryOption itinerary={itinerary} selected={false} onSelect={vi.fn()} />);
    expect(screen.getByText('Rail replacement bus')).toBeInTheDocument();
    expect(screen.getByText('Ferry')).toBeInTheDocument();
    expect(screen.queryByText('Bus service')).not.toBeInTheDocument();
    // A bus/ferry leg is still a selectable, trackable leg.
    expect(screen.getByRole('radio')).not.toBeDisabled();
  });
});

describe('ItineraryOption: station groups (2026-10-07)', () => {
  const london = {
    group: 'LON',
    code: 'group:LON',
    name: 'London Terminals',
    members: [{ crs: 'EUS', name: 'London Euston' }],
  };

  it('names the member a group via and a group stop used', () => {
    renderWithMantine(
      <ItineraryOption
        itinerary={trainItinerary}
        selected={false}
        onSelect={vi.fn()}
        stationNames={new Map([['EUS', 'London Euston']])}
        viaPasses={[{ crs: 'group:LON', matchedCrs: 'EUS', segment: 0, leg: 0, how: 'call' }]}
        waypointStops={[{ crs: 'group:LON', matchedCrs: 'EUS', segment: 0, how: 'origin' }]}
        groups={[london]}
      />,
    );
    expect(screen.getByText('Calls at London Euston (one of the London Terminals)')).toBeInTheDocument();
    expect(screen.getByText('Starts at London Euston (one of the London Terminals)')).toBeInTheDocument();
  });

  it('keeps a single-station via as it was', () => {
    renderWithMantine(
      <ItineraryOption
        itinerary={trainItinerary}
        selected={false}
        onSelect={vi.fn()}
        viaPasses={[{ crs: 'MKC', matchedCrs: 'MKC', segment: 0, leg: 0, how: 'call' }]}
      />,
    );
    expect(screen.getByText('Calls at MKC')).toBeInTheDocument();
  });

  it('names a TfL stop end and says its access leg is an estimate (DS-135)', () => {
    const uxbridge = {
      id: '940GZZLUUXB',
      name: 'Uxbridge',
      modes: ['tube'],
      lines: [{ id: 'metropolitan', name: 'Metropolitan' }],
    };
    const harrow = { ...uxbridge, id: '940GZZLUHOH', name: 'Harrow-on-the-Hill' };
    renderWithMantine(
      <ItineraryOption
        itinerary={{
          legs: [
            {
              kind: 'transfer',
              mode: 'TUBE',
              originCrs: null,
              destinationCrs: 'HOH',
              minutes: 30,
              line: 'Metropolitan',
              originStop: uxbridge,
              destinationStop: harrow,
              access: { end: 'origin', basis: 'line', distanceMetres: 11_400 },
            },
            {
              ...trainItinerary.legs[0]!,
              originCrs: 'HOH',
              destinationCrs: 'MYB',
            },
          ],
          changeCount: 0,
          totalDurationMinutes: 70,
        }}
        selected={false}
        onSelect={vi.fn()}
        stationNames={new Map([['HOH', 'Harrow-on-the-Hill']])}
      />,
    );
    expect(
      screen.getByText(
        byVisibleText(/Tube \(Metropolitan line\) Uxbridge \(Tube\) to HOH · Harrow-on-the-Hill, 30 min/),
      ),
    ).toBeInTheDocument();
    expect(screen.getByText('Estimated time for about 11.4 km along the line')).toBeInTheDocument();
    // The itinerary has a train, so it can be chosen.
    expect(screen.getByRole('radio')).not.toBeDisabled();
  });

  it('says a walk to a nearby station is an estimate', () => {
    renderWithMantine(
      <ItineraryOption
        itinerary={{
          legs: [
            { ...trainItinerary.legs[0]!, destinationCrs: 'WRU' },
            {
              kind: 'transfer',
              mode: 'WALK',
              originCrs: 'WRU',
              destinationCrs: null,
              minutes: 12,
              destinationStop: { id: '940GZZLURSM', name: 'Ruislip', modes: ['tube'], lines: [] },
              access: { end: 'destination', basis: 'walk', distanceMetres: 650 },
            },
          ],
          changeCount: 0,
          totalDurationMinutes: 62,
        }}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText(byVisibleText(/Walk WRU to Ruislip \(Tube\), 12 min/))).toBeInTheDocument();
    expect(screen.getByText('Estimated walk: the station is about 0.7 km away')).toBeInTheDocument();
  });
});
