import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrainJourney } from './TrainJourney';
import type { TrackedTrainState } from '@/lib/types';

function baseState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-08-28',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'pending',
    trainUid: null,
    trainId: null,
    status: null,
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: null,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: null,
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    ...overrides,
    customName: overrides.customName ?? null,
  };
}

describe('TrainJourney', () => {
  it('pending: shows a waiting panel with the pinned origin/destination/date', () => {
    renderWithMantine(<TrainJourney state={baseState()} />);
    expect(screen.getByText('Waiting to hear from Network Rail')).toBeInTheDocument();
    expect(screen.getByText(/WAT/)).toBeInTheDocument();
    expect(screen.getByText(/WOK/)).toBeInTheDocument();
  });

  it('renders station names in the pin summary when the backend resolved them', () => {
    renderWithMantine(
      <TrainJourney state={baseState({ pinOriginName: 'London Waterloo', pinDestinationName: 'Woking' })} />,
    );
    expect(screen.getByText(/London Waterloo \(WAT\) → Woking \(WOK\)/)).toBeInTheDocument();
  });

  it('falls back to the bare code, not "null", when a name did not resolve', () => {
    renderWithMantine(<TrainJourney state={baseState()} />);
    expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
    expect(screen.queryByText(/null/i)).not.toBeInTheDocument();
  });

  it('schedule_matched: names the matched train and destination, with a caveat badge', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'schedule_matched',
          trainUid: 'C88888',
          scheduleDestinationCrs: 'CRE',
          scheduleDestinationName: 'Crewe',
        })}
      />,
    );
    expect(screen.getByText(/Matched to a scheduled service — Train C88888 to Crewe/)).toBeInTheDocument();
    expect(screen.getByText('As scheduled')).toBeInTheDocument();
    expect(screen.getByText(/Waiting for Network Rail's live tracking to begin/)).toBeInTheDocument();
  });

  it('schedule_matched: falls back to the destination CRS when no name resolved, and omits it entirely when neither did', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'schedule_matched',
          trainUid: 'C88888',
          scheduleDestinationCrs: 'CRE',
        })}
      />,
    );
    expect(screen.getByText(/Train C88888 to CRE/)).toBeInTheDocument();

    renderWithMantine(
      <TrainJourney state={baseState({ resolutionStatus: 'schedule_matched', trainUid: 'C88888' })} />,
    );
    expect(screen.getAllByText(/Train C88888/).length).toBeGreaterThan(0);
  });

  it('unresolved: shows a terminal, non-retrying message', () => {
    renderWithMantine(<TrainJourney state={baseState({ resolutionStatus: 'unresolved' })} />);
    expect(screen.getByText("Couldn't be matched to a live service")).toBeInTheDocument();
    expect(screen.getByText(/won't resolve on its own/)).toBeInTheDocument();
  });

  it('resolved + awaiting_activation: names the matched train, no movement data', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({ resolutionStatus: 'resolved', trainUid: 'C21373', status: 'awaiting_activation' })}
      />,
    );
    expect(screen.getByText('Matched to train C21373')).toBeInTheDocument();
    expect(screen.getByText('Waiting for its first movement report.')).toBeInTheDocument();
  });

  it('resolved + en_route: shows location, delay, next calling point, and ETA', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'resolved',
          trainUid: 'C21373',
          status: 'en_route',
          lastReportedLocation: 'Clapham Junction',
          lastEventType: 'DEPARTURE',
          delayMinutes: 4,
          nextCallingPoint: 'Woking',
          etaNext: '2026-08-28T18:41:00Z',
          etaSource: 'trust-propagated',
        })}
      />,
    );
    expect(screen.getByText(/Clapham Junction/)).toBeInTheDocument();
    expect(screen.getByText('4m late')).toBeInTheDocument();
    expect(screen.getByText('Next calling point: Woking')).toBeInTheDocument();
    expect(screen.getByText(/ETA/)).toBeInTheDocument();
    expect(screen.queryByText('May have finished')).not.toBeInTheDocument();
  });

  it('resolved + en_route with no next calling point: shows the provisional "may have finished" note', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'resolved',
          trainUid: 'C21373',
          status: 'en_route',
          lastReportedLocation: 'Woking',
          nextCallingPoint: null,
        })}
      />,
    );
    expect(screen.getByText('May have finished')).toBeInTheDocument();
    expect(screen.getByText(/this is an inference, not a confirmed status/)).toBeInTheDocument();
  });

  it('resolved + cancelled: shows a cancelled banner and retains last known location', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'resolved',
          trainUid: 'C21373',
          status: 'cancelled',
          lastReportedLocation: 'Surbiton',
        })}
      />,
    );
    expect(screen.getByText('Cancelled')).toBeInTheDocument();
    expect(screen.getByText(/Surbiton/)).toBeInTheDocument();
  });

  it('resolved + en_route with no movement fields at all: shows a "no movement data" fallback', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'resolved',
          trainUid: 'C21373',
          status: 'en_route',
        })}
      />,
    );
    expect(screen.getByText('No movement data reported yet.')).toBeInTheDocument();
  });

  it('resolved + completed: shows the same arrived treatment as the no-next-stop en_route case', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'resolved',
          trainUid: 'C21373',
          status: 'completed',
          lastReportedLocation: 'Woking',
        })}
      />,
    );
    expect(screen.getByText('May have finished')).toBeInTheDocument();
  });
});
