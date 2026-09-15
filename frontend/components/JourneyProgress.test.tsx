import { fireEvent, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MantineProvider } from '@mantine/core';
import { renderWithMantine } from '@/test/render';
import { theme } from '@/lib/theme';
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

  it('carries a role="img" and a "currently at" aria-label once a marker exists', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'WAT', kind: 'Origin' }), stop({ crs: 'WOK', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="awaiting_activation"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    expect(
      screen.getByRole('img', { name: /Journey progress: matched to train/ }),
    ).toBeInTheDocument();
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

  it('cancelled: nodes after the frozen marker render in a distinct cancelled style, not the plain not-yet-reached style', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Intermediate' }),
          stop({ crs: 'C', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="cancelled"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(nodes[0]).toHaveAttribute('data-node-state', 'marker');
    expect(nodes[1]).toHaveAttribute('data-node-state', 'cancelled-remaining');
    expect(nodes[2]).toHaveAttribute('data-node-state', 'cancelled-remaining');
  });

  it('cancelled before any confirmed movement: every node is cancelled-remaining, none is a marker', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="cancelled"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(Array.from(nodes).every((n) => n.getAttribute('data-node-state') === 'cancelled-remaining')).toBe(true);
  });

  it('completed: no node anywhere carries the cancelled-remaining style', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Terminate', actualArrival: '2026-09-12T09:00:00Z' }),
        ]}
        resolutionStatus="resolved"
        status="completed"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(Array.from(nodes).some((n) => n.getAttribute('data-node-state') === 'cancelled-remaining')).toBe(false);
  });

  it('mayHaveArrived: the marker carries a distinguishing badge attribute, with its fill color unaffected', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z', delayMinutes: 0 }),
          stop({ crs: 'B', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={true}
      />,
    );
    expect(container.querySelector('[data-may-have-arrived="true"]')).toBeInTheDocument();
    const marker = container.querySelector('[data-node-state="marker"]');
    expect(marker).toHaveAttribute('data-delay-state', 'on-time');
  });

  it('mayHaveArrived false: no badge renders anywhere', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(container.querySelector('[data-may-have-arrived]')).not.toBeInTheDocument();
  });

  it('every decorative node circle is aria-hidden, but an intermediate node\'s focusable Tooltip trigger is not', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', name: 'Alpha', kind: 'Origin' }),
          stop({ crs: 'B', name: 'Bravo', kind: 'Intermediate' }),
          stop({ crs: 'C', name: 'Charlie', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    const circles = container.querySelectorAll('[data-journey-node]');
    circles.forEach((circle) => expect(circle).toHaveAttribute('aria-hidden', 'true'));
    const trigger = screen.getByLabelText('Bravo');
    expect(trigger).not.toHaveAttribute('aria-hidden');
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

describe('JourneyProgress auto-scroll', () => {
  beforeEach(() => {
    window.HTMLElement.prototype.scrollIntoView = vi.fn();
    window.Element.prototype.scrollTo = vi.fn();
  });

  it('scrolls its own scroll container, centering the marker, on mount when a marker exists', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    const scroller = container.querySelector('[data-journey-progress-scroll]');
    expect(scroller).toBeInTheDocument();
    expect(window.Element.prototype.scrollTo).toHaveBeenCalledWith(
      expect.objectContaining({ left: expect.any(Number), behavior: 'smooth' }),
    );
    expect((window.Element.prototype.scrollTo as ReturnType<typeof vi.fn>).mock.instances[0]).toBe(scroller);
  });

  // The mobile-layout regression this component shipped with: `scrollIntoView`
  // scrolls EVERY scrollable ancestor including the document, and defaults
  // `block` to `'start'`, so on a phone (where the diagram is below the fold)
  // opening a train page yanked the whole page down to the diagram.
  it('never scrolls the page itself -- no scrollIntoView call reaches the document', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Intermediate', actualArrival: '2026-09-12T08:30:00Z' }),
          stop({ crs: 'C', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(window.HTMLElement.prototype.scrollIntoView).not.toHaveBeenCalled();
  });

  it('does not scroll at all when there is no marker (lastReachedIndex === -1)', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="awaiting_activation"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(window.Element.prototype.scrollTo).not.toHaveBeenCalled();
    expect(window.HTMLElement.prototype.scrollIntoView).not.toHaveBeenCalled();
  });

  it('uses an instant jump, not smooth scrolling, when the viewer prefers reduced motion', () => {
    vi.spyOn(window, 'matchMedia').mockImplementation(
      (query: string) =>
        ({
          matches: query === '(prefers-reduced-motion: reduce)',
          media: query,
          onchange: null,
          addListener: vi.fn(),
          removeListener: vi.fn(),
          addEventListener: vi.fn(),
          removeEventListener: vi.fn(),
          dispatchEvent: vi.fn(),
        }) as unknown as MediaQueryList,
    );

    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(window.Element.prototype.scrollTo).toHaveBeenCalledWith(
      expect.objectContaining({ behavior: 'auto' }),
    );
  });

  it('re-scrolls when lastReachedIndex advances on rerender (a new confirmed event moved the marker)', () => {
    const { rerender } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Intermediate' }),
          stop({ crs: 'C', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    (window.Element.prototype.scrollTo as ReturnType<typeof vi.fn>).mockClear();

    rerender(
      <MantineProvider theme={theme}>
        <JourneyProgress
          stops={[
            stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
            stop({ crs: 'B', kind: 'Intermediate', actualArrival: '2026-09-12T08:15:00Z' }),
            stop({ crs: 'C', kind: 'Terminate' }),
          ]}
          resolutionStatus="resolved"
          status="en_route"
          trainUid="C1"
          mayHaveArrived={false}
        />
      </MantineProvider>,
    );
    expect(window.Element.prototype.scrollTo).toHaveBeenCalledTimes(1);
  });
});

// The diagram's whole horizontal scale (slot width, endpoint slot width) is
// declared in `app/globals.css` so a `max-width: $mantine-breakpoint-sm`
// media query can rescale it for a phone -- a media query cannot reach into
// a React style object. These assert the contract that makes that possible:
// the component contributes only the data-derived COUNTS inline, and every
// px measurement stays in the stylesheet.
describe('JourneyProgress responsive layout contract', () => {
  const threeStops = [
    stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Origin' }),
    stop({ crs: 'CLJ', name: 'Clapham Junction', kind: 'Intermediate' }),
    stop({ crs: 'WOK', name: 'Woking', kind: 'Terminate' }),
  ];

  function renderThree() {
    return renderWithMantine(
      <JourneyProgress
        stops={threeStops}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
  }

  it('publishes the stop count and the endpoint count as CSS custom properties on the scroll box', () => {
    const { container } = renderThree();
    const scroller = container.querySelector<HTMLElement>('[data-journey-progress-scroll]');
    expect(scroller).toBeInTheDocument();
    expect(scroller!.style.getPropertyValue('--journey-progress-count')).toBe('3');
    expect(scroller!.style.getPropertyValue('--journey-progress-endpoint-count')).toBe('2');
  });

  it('counts endpoints honestly when a journey has no Origin/Terminate stop at all', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Intermediate' }), stop({ crs: 'B', kind: 'Intermediate' })]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    const scroller = container.querySelector<HTMLElement>('[data-journey-progress-scroll]');
    expect(scroller!.style.getPropertyValue('--journey-progress-count')).toBe('2');
    expect(scroller!.style.getPropertyValue('--journey-progress-endpoint-count')).toBe('0');
  });

  it('leaves every horizontal px measurement to the stylesheet -- no inline width/flex on the scroll box or any node', () => {
    const { container } = renderThree();
    const scroller = container.querySelector<HTMLElement>('[data-journey-progress-scroll]')!;
    expect(scroller.style.minWidth).toBe('');
    expect(scroller.style.width).toBe('');

    const line = container.querySelector<HTMLElement>('.journeyProgressLine')!;
    expect(line.style.minWidth).toBe('');

    container.querySelectorAll<HTMLElement>('.journeyProgressNode').forEach((node) => {
      expect(node.style.flex).toBe('');
      expect(node.style.width).toBe('');
      expect(node.style.minWidth).toBe('');
    });
  });

  it('gives every node the shared slot class, with the wider modifier on endpoints only', () => {
    const { container } = renderThree();
    const nodes = container.querySelectorAll('.journeyProgressNode');
    expect(nodes).toHaveLength(3);
    expect(nodes[0]).toHaveClass('journeyProgressNode--endpoint');
    expect(nodes[1]).not.toHaveClass('journeyProgressNode--endpoint');
    expect(nodes[2]).toHaveClass('journeyProgressNode--endpoint');
  });

  it("constrains an endpoint's always-visible label to its slot so it can't widen the row", () => {
    const { container } = renderThree();
    const labels = container.querySelectorAll('.journeyProgressLabel');
    expect(labels).toHaveLength(2);
    expect(labels[0]).toHaveTextContent('London Waterloo');
    expect(labels[1]).toHaveTextContent('Woking');
  });

  it("gives an intermediate node's Tooltip trigger the enlarged tap-target class", () => {
    renderThree();
    expect(screen.getByLabelText('Clapham Junction')).toHaveClass('journeyProgressTrigger');
  });
});

describe('JourneyProgress decision-table captions and aria-labels', () => {
  it('schedule_matched: no marker, "scheduled route" caption and aria-label', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="schedule_matched"
        status={null}
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText("Scheduled route shown — live tracking hasn't started yet.")).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Journey progress: scheduled route shown, live tracking not yet started' }),
    ).toBeInTheDocument();
  });

  it('resolved + awaiting_activation: no marker, "matched to train" caption naming trainUid', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="awaiting_activation"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    expect(
      screen.getByText('Matched to train C21373 — waiting for its first movement report.'),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('img', {
        name: 'Journey progress: matched to train C21373, waiting for first movement report',
      }),
    ).toBeInTheDocument();
  });

  it('resolved + en_route, mayHaveArrived false: "Currently at X" caption and matching aria-label', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', name: 'Alpha', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', name: 'Bravo', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText('Currently at Alpha.')).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Journey progress: currently at Alpha, stop 1 of 2' }),
    ).toBeInTheDocument();
  });

  it('resolved + en_route, mayHaveArrived true: same caption, aria-label notes the inference', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', name: 'Alpha', kind: 'Origin' }),
          stop({ crs: 'B', name: 'Bravo', kind: 'Terminate', actualArrival: '2026-09-12T09:00:00Z' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={true}
      />,
    );
    expect(screen.getByText('Currently at Bravo.')).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Journey progress: currently at Bravo (may have arrived), stop 2 of 2' }),
    ).toBeInTheDocument();
  });

  it('resolved + cancelled, with a confirmed marker: "Cancelled — last confirmed at X" caption/aria-label', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', name: 'Alpha', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', name: 'Bravo', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="cancelled"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText('Cancelled — last confirmed at Alpha.')).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Journey progress: cancelled, last confirmed at Alpha, stop 1 of 2' }),
    ).toBeInTheDocument();
  });

  it('resolved + cancelled, before any confirmed movement: a distinct caption/aria-label', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="cancelled"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText('Cancelled — no movement was ever confirmed.')).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Journey progress: cancelled before any confirmed movement' }),
    ).toBeInTheDocument();
  });

  it('resolved + completed: "Arrived at X" caption/aria-label naming the terminus', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', name: 'Alpha', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', name: 'Bravo', kind: 'Terminate', actualArrival: '2026-09-12T09:00:00Z' }),
        ]}
        resolutionStatus="resolved"
        status="completed"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText('Arrived at Bravo.')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Journey progress: arrived at Bravo' })).toBeInTheDocument();
  });

  it('empty stops array: "Not yet started" caption/aria-label, defensively (not reachable via the real TrainJourney guard, but must not crash)', () => {
    renderWithMantine(
      <JourneyProgress stops={[]} resolutionStatus="resolved" status="en_route" trainUid="C1" mayHaveArrived={false} />,
    );
    expect(screen.getByText('Not yet started.')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Journey progress: not yet started' })).toBeInTheDocument();
  });
});
