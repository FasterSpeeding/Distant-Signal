import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { JourneyProgress } from './JourneyProgress';
import type { JourneyStop } from '@/lib/types';

function stop(overrides: Partial<JourneyStop>): JourneyStop {
  return {
    crs: 'RDG',
    name: 'Reading',
    tiploc: null,
    kind: 'Intermediate',
    scheduledArrival: null,
    scheduledDeparture: null,
    actualArrival: null,
    actualDeparture: null,
    estimatedArrival: null,
    estimatedDeparture: null,
    lastEventType: null,
    variationStatus: null,
    delayMinutes: null,
    ...overrides,
  };
}

describe('JourneyProgress', () => {
  it('renders one node per stop, in order', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Origin' }),
          stop({ crs: 'CLJ', name: 'Clapham Junction', kind: 'Intermediate' }),
          stop({ crs: 'WOK', name: 'Woking', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    expect(container.querySelectorAll('[data-journey-node]')).toHaveLength(3);
  });

  it('renders Origin and Terminate nodes at a larger diameter than an Intermediate node', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'WAT', kind: 'Origin' }),
          stop({ crs: 'CLJ', kind: 'Intermediate' }),
          stop({ crs: 'WOK', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect((nodes[0] as HTMLElement).style.width).toBe('18px');
    expect((nodes[1] as HTMLElement).style.width).toBe('12px');
    expect((nodes[2] as HTMLElement).style.width).toBe('18px');
  });

  it('renders no nodes and does not crash for an empty stops array', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    expect(container.querySelectorAll('[data-journey-node]')).toHaveLength(0);
    expect(screen.getByRole('img')).toBeInTheDocument();
  });

  it('carries a role="img" and a stop-count aria-label before any marker logic exists', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'WAT', kind: 'Origin' }), stop({ crs: 'WOK', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByRole('img', { name: 'Journey progress: 2 stops' })).toBeInTheDocument();
  });
});
