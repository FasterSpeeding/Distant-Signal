import { screen, fireEvent } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { ItineraryOption } from './ItineraryOption';
import type { TripPlanItinerary } from '@/lib/types';

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
    expect(screen.getByText(/08:00 EUS → MKC 08:50/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('radio'));
    expect(onSelect).toHaveBeenCalled();
  });

  it('flags an itinerary that exceeds the recommended change count', () => {
    renderWithMantine(
      <ItineraryOption
        itinerary={{ ...trainItinerary, changeCount: 3, exceedsRecommendedChanges: true }}
        selected={false}
        onSelect={vi.fn()}
      />
    );
    expect(screen.getByText('More changes than usually recommended')).toBeInTheDocument();
  });

  it('disables selection for a fixed-link-only itinerary with no train leg', () => {
    renderWithMantine(<ItineraryOption itinerary={fixedLinkOnlyItinerary} selected={false} onSelect={vi.fn()} />);
    expect(screen.getByRole('radio')).toBeDisabled();
    expect(screen.getByText('This route needs no train — there is nothing to track.')).toBeInTheDocument();
  });

  // The bug this fixes: `GET /Trips/plan` never sends a name alongside a
  // leg's bare `originCrs`/`destinationCrs` (unlike every other
  // station-bearing response in this app), so this line used to render
  // ONLY the raw codes. `PlanTripFlow` (the only real caller) resolves
  // names itself and passes them down via `stationNames`; this proves the
  // leg summary renders `CODE — Name` once a name is available, for both
  // a train leg and a transfer leg.
  it('renders "CODE — Name" for each leg end once stationNames resolves a name', () => {
    const stationNames = new Map([
      ['EUS', 'London Euston'],
      ['MKC', 'Milton Keynes Central'],
    ]);
    renderWithMantine(
      <ItineraryOption itinerary={trainItinerary} selected={false} onSelect={vi.fn()} stationNames={stationNames} />,
    );
    expect(screen.getByText('08:00 EUS — London Euston → MKC — Milton Keynes Central 08:50')).toBeInTheDocument();
  });

  it('renders "CODE — Name" for a transfer leg once stationNames resolves a name', () => {
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
    expect(screen.getByText('Walk/transfer (TUBE) EUS — London Euston → KGX — London Kings Cross, 5 min')).toBeInTheDocument();
  });

  it('falls back to bare codes for an end whose name did not resolve, without mixing forms', () => {
    // Only EUS resolved -- per `codeRouteLabel`'s own "never mix" rule
    // (mirroring `routeLabel`'s), BOTH ends must render as bare codes
    // rather than "EUS — London Euston → MKC".
    const stationNames = new Map([['EUS', 'London Euston']]);
    renderWithMantine(
      <ItineraryOption itinerary={trainItinerary} selected={false} onSelect={vi.fn()} stationNames={stationNames} />,
    );
    expect(screen.getByText('08:00 EUS → MKC 08:50')).toBeInTheDocument();
  });

  it('renders bare codes when stationNames is omitted (default empty map)', () => {
    renderWithMantine(<ItineraryOption itinerary={trainItinerary} selected={false} onSelect={vi.fn()} />);
    expect(screen.getByText('08:00 EUS → MKC 08:50')).toBeInTheDocument();
  });
});
