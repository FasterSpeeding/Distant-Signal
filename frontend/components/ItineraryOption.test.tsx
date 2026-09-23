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
});
