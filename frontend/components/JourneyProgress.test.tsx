import { fireEvent, screen } from '@testing-library/react';
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

  it('marks the highest-index stop with a confirmed actualArrival/actualDeparture as the marker', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'WAT', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({
            crs: 'CLJ',
            kind: 'Intermediate',
            actualArrival: '2026-09-12T08:20:00Z',
            actualDeparture: '2026-09-12T08:21:00Z',
          }),
          stop({ crs: 'WOK', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(nodes[0]).toHaveAttribute('data-node-state', 'reached');
    expect(nodes[1]).toHaveAttribute('data-node-state', 'marker');
    expect(nodes[2]).toHaveAttribute('data-node-state', 'not-reached');
  });

  it('renders no marker at all when nothing has been confirmed yet (lastReachedIndex === -1)', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'WAT', kind: 'Origin' }), stop({ crs: 'WOK', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="awaiting_activation"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(Array.from(nodes).every((n) => n.getAttribute('data-node-state') === 'not-reached')).toBe(true);
  });

  it('colors a reached node green/orange/teal by delayMinutes, and gray when delayMinutes is unknown', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z', delayMinutes: 0 }),
          stop({ crs: 'B', kind: 'Intermediate', actualArrival: '2026-09-12T08:10:00Z', delayMinutes: 4 }),
          stop({ crs: 'C', kind: 'Intermediate', actualArrival: '2026-09-12T08:20:00Z', delayMinutes: -2 }),
          stop({ crs: 'D', kind: 'Terminate', actualArrival: '2026-09-12T08:30:00Z', delayMinutes: null }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(nodes[0]).toHaveAttribute('data-delay-state', 'on-time');
    expect(nodes[1]).toHaveAttribute('data-delay-state', 'late');
    expect(nodes[2]).toHaveAttribute('data-delay-state', 'early');
    // nodes[3] is also the marker (last confirmed index) -- delay unknown.
    expect(nodes[3]).toHaveAttribute('data-delay-state', 'unknown');
  });

  it('a PASS event (both actualArrival and actualDeparture set to the same instant) still counts that stop as reached', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin' }),
          stop({
            crs: 'B',
            kind: 'Intermediate',
            actualArrival: '2026-09-12T08:10:00Z',
            actualDeparture: '2026-09-12T08:10:00Z',
          }),
          stop({ crs: 'C', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(nodes[1]).toHaveAttribute('data-node-state', 'marker');
  });

  it('always shows the origin and terminus station names as visible text, but not an intermediate node\'s', () => {
    renderWithMantine(
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
    expect(screen.getByText('London Waterloo')).toBeInTheDocument();
    expect(screen.getByText('Woking')).toBeInTheDocument();
    expect(screen.queryByText('Clapham Junction')).not.toBeInTheDocument();
  });

  it('reveals an intermediate node\'s name and scheduled time via Tooltip on hover', async () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Origin' }),
          stop({
            crs: 'CLJ',
            name: 'Clapham Junction',
            kind: 'Intermediate',
            scheduledArrival: '2026-09-12T08:15:00Z',
          }),
          stop({ crs: 'WOK', name: 'Woking', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const trigger = screen.getByLabelText(/Clapham Junction/);
    fireEvent.mouseEnter(trigger);
    // 2026-09-12 is within BST (UTC+1) -- 08:15Z renders as 09:15 London
    // time, same `formatTime`/Europe-London posture `JourneyTimeline` uses.
    expect(await screen.findByText('09:15')).toBeInTheDocument();
  });

  it('reveals a bare node\'s name even with no scheduled time known', async () => {
    renderWithMantine(
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
    const trigger = screen.getByLabelText('Clapham Junction');
    fireEvent.mouseEnter(trigger);
    expect(await screen.findByText('Clapham Junction')).toBeInTheDocument();
  });
});
