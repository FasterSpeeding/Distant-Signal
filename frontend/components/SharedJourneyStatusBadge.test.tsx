import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { SharedJourneyStatusBadge } from './SharedJourneyStatusBadge';
import type { GroupJourney, UntrackedStatus } from '@/lib/types';

function journey(overrides: Partial<GroupJourney> = {}): GroupJourney {
  return {
    journeyId: 1,
    customName: null,
    legCount: 2,
    pinOriginCrs: null,
    pinDestinationCrs: null,
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: null,
    serviceDate: '2026-10-21',
    resolutionStatus: 'schedule_matched',
    trainUid: 'P00001',
    status: null,
    delayMinutes: null,
    addedBy: 'user-1',
    addedByName: 'Sam',
    addedByTag: null,
    kind: 'train',
    ...overrides,
  };
}

function untracked(untrackedStatus: UntrackedStatus | null): GroupJourney {
  return journey({
    kind: 'untracked',
    transitMode: 'tube',
    plannedDeparture: '2026-10-21T08:00:00Z',
    plannedArrival: '2026-10-21T08:20:00Z',
    untrackedStatus,
    resolutionStatus: null,
    trainUid: null,
  });
}

describe('SharedJourneyStatusBadge', () => {
  it.each([
    ['planned', 'Planned'],
    ['inProgress', 'In progress (not live tracked)'],
    ['done', 'Done'],
  ] as const)('an untracked-only journey that is %s reads "%s"', (status, label) => {
    renderWithMantine(<SharedJourneyStatusBadge journey={untracked(status)} />);
    expect(screen.getByText(label)).toBeInTheDocument();
    expect(screen.queryByText('Unmatched')).not.toBeInTheDocument();
  });

  it('an untracked leg with no status yet (older backend) reads as planned', () => {
    renderWithMantine(<SharedJourneyStatusBadge journey={untracked(null)} />);
    expect(screen.getByText('Planned')).toBeInTheDocument();
  });

  it('a train leg with no train picked asks for one', () => {
    renderWithMantine(<SharedJourneyStatusBadge journey={journey({ resolutionStatus: null, trainUid: null })} />);
    expect(screen.getByText('Needs a train picked')).toBeInTheDocument();
    expect(screen.queryByText('Unmatched')).not.toBeInTheDocument();
  });

  it('a planned train reads "Planned", not "Matched to schedule"', () => {
    renderWithMantine(<SharedJourneyStatusBadge journey={journey({ trackingPhase: 'planned' })} />);
    expect(screen.getByText('Planned')).toBeInTheDocument();
    expect(screen.queryByText('Matched to schedule')).not.toBeInTheDocument();
  });

  it('a train with no timetable reads "Not in the timetable"', () => {
    renderWithMantine(
      <SharedJourneyStatusBadge journey={journey({ resolutionStatus: 'pending', trackingPhase: 'noTimetable' })} />,
    );
    expect(screen.getByText('Not in the timetable')).toBeInTheDocument();
    expect(screen.queryByText('Pending match')).not.toBeInTheDocument();
  });

  it('a bus reads "Bus service" and "Timetabled only"', () => {
    renderWithMantine(
      <SharedJourneyStatusBadge
        journey={journey({ trackingPhase: 'timetableOnly', serviceMode: 'bus', liveTracking: false })}
      />,
    );
    expect(screen.getByText('Bus service')).toBeInTheDocument();
    expect(screen.getByText('Timetabled only')).toBeInTheDocument();
  });

  it('a live train shows its status and delay', () => {
    renderWithMantine(
      <SharedJourneyStatusBadge
        journey={journey({ resolutionStatus: 'resolved', status: 'en_route', delayMinutes: 4, trackingPhase: 'live' })}
      />,
    );
    expect(screen.getByText('En route')).toBeInTheDocument();
    expect(screen.getByText('4 min late')).toBeInTheDocument();
  });
});
