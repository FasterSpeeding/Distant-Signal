import { act, fireEvent, screen } from '@testing-library/react';
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
    expect(screen.getByRole('group')).toBeInTheDocument();
  });

  it('carries a role="group" and a "currently at" aria-label once a marker exists', () => {
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
      screen.getByRole('group', { name: /Journey progress: matched to train/ }),
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

  // Regression guard for the keyboard trap the mobile-scaling review found:
  // the diagram's container used to be `role="img"`, whose subtree ARIA
  // treats as presentational, so these focusable Tooltip triggers were in
  // the tab order but announced nothing (WCAG 2.1.1 + 4.1.2). The fix is
  // BOTH halves together -- a `group` container, and triggers that carry a
  // real role plus an accessible name -- so these tests assert both.
  describe('keyboard reachability of the per-node Tooltip triggers', () => {
    /** A deliberately crude stand-in for the real accname algorithm (jsdom
     * implements none of it): the element's own `aria-label`, else its
     * text content with every `aria-hidden` subtree removed. Enough to
     * tell "has some name" from "has none", which is all these assertions
     * need. */
    function accessibleName(el: HTMLElement): string {
      const label = el.getAttribute('aria-label');
      if (label !== null) return label.trim();
      const clone = el.cloneNode(true) as HTMLElement;
      clone.querySelectorAll('[aria-hidden="true"]').forEach((hidden) => hidden.remove());
      return (clone.textContent ?? '').trim();
    }

    function renderThreeStopJourney() {
      return renderWithMantine(
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
    }

    it('labels the diagram container as a group, never as a presentational img', () => {
      const { container } = renderThreeStopJourney();
      expect(container.querySelector('[role="img"]')).not.toBeInTheDocument();
      expect(screen.getByRole('group', { name: /^Journey progress: / })).toBeInTheDocument();
    });

    it('exposes each intermediate node trigger as a named button, not an anonymous focusable div', () => {
      const { container } = renderThreeStopJourney();
      const trigger = screen.getByRole('button', { name: 'Bravo' });
      expect(trigger.tagName).toBe('BUTTON');
      // Not a submit button: harmless today (no ancestor <form>), but this
      // is exactly the attribute a later refactor drops silently.
      expect(trigger).toHaveAttribute('type', 'button');
      // Spec Decision 6 wants these reachable, so the trigger must appear
      // in the document's own tab order -- asserted by collecting it, not
      // just by checking `tabindex !== "-1"` on the element in hand
      // (`getByRole`/`.focus()` would both still pass for a
      // `tabindex="-1"` element, which is reachable by script but not by
      // the Tab key).
      const tabbable = Array.from(
        container.querySelectorAll<HTMLElement>('button, [href], input, select, textarea, [tabindex]'),
      ).filter((el) => el.getAttribute('tabindex') !== '-1' && !el.hasAttribute('disabled'));
      expect(tabbable).toContain(trigger);
      // `act` because focusing opens the Tooltip, which is a state update.
      act(() => trigger.focus());
      expect(trigger).toHaveFocus();
    });

    it('gives an accessible name to every element left in the tab order', () => {
      const { container } = renderThreeStopJourney();
      const focusable = Array.from(
        container.querySelectorAll<HTMLElement>('button, [href], input, select, textarea, [tabindex]'),
      ).filter((el) => el.getAttribute('tabindex') !== '-1');
      expect(focusable.length).toBeGreaterThan(0);
      for (const el of focusable) {
        // Either its own label or its text content, minus any `aria-hidden`
        // subtree (text a reader never speaks can't be the element's name)
        // -- an empty name here is exactly the "focusable but silent"
        // defect being guarded against.
        const name = accessibleName(el);
        expect(name, `${el.outerHTML} has no accessible name`).not.toBe('');
        // And nothing in the tab order may sit inside a subtree ARIA drops
        // (`aria-hidden`, or a presentational `role="img"`/`role="presentation"`).
        expect(el.closest('[aria-hidden="true"], [role="img"], [role="presentation"], [role="none"]')).toBeNull();
      }
    });

    it('adds no tab stop for an endpoint node, whose name is already visible text', () => {
      renderThreeStopJourney();
      // One button for the single Intermediate stop; Origin/Terminate print
      // their names instead of hiding them behind a focusable tooltip.
      expect(screen.getAllByRole('button')).toHaveLength(1);
      expect(screen.queryByRole('button', { name: 'Alpha' })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Charlie' })).not.toBeInTheDocument();
    });

    it('opens the tooltip on keyboard focus and describes the trigger while it is open', async () => {
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
      const trigger = screen.getByRole('button', { name: 'Clapham Junction' });
      fireEvent.focus(trigger);
      expect(await screen.findByText('09:15')).toBeInTheDocument();
      expect(trigger).toHaveAttribute('aria-describedby');
    });
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
  });

  it('scrolls the marker node into view, centered, on mount when a marker exists', () => {
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
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenCalledWith(
      expect.objectContaining({ inline: 'center', behavior: 'smooth' }),
    );
  });

  it('does not call scrollIntoView when there is no marker (lastReachedIndex === -1)', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="awaiting_activation"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
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
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenCalledWith(
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
    (window.HTMLElement.prototype.scrollIntoView as ReturnType<typeof vi.fn>).mockClear();

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
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenCalledTimes(1);
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
      screen.getByRole('group', { name: 'Journey progress: scheduled route shown, live tracking not yet started' }),
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
      screen.getByRole('group', {
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
      screen.getByRole('group', { name: 'Journey progress: currently at Alpha, stop 1 of 2' }),
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
      screen.getByRole('group', { name: 'Journey progress: currently at Bravo (may have arrived), stop 2 of 2' }),
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
      screen.getByRole('group', { name: 'Journey progress: cancelled, last confirmed at Alpha, stop 1 of 2' }),
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
      screen.getByRole('group', { name: 'Journey progress: cancelled before any confirmed movement' }),
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
    expect(screen.getByRole('group', { name: 'Journey progress: arrived at Bravo' })).toBeInTheDocument();
  });

  it('empty stops array: "Not yet started" caption/aria-label, defensively (not reachable via the real TrainJourney guard, but must not crash)', () => {
    renderWithMantine(
      <JourneyProgress stops={[]} resolutionStatus="resolved" status="en_route" trainUid="C1" mayHaveArrived={false} />,
    );
    expect(screen.getByText('Not yet started.')).toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'Journey progress: not yet started' })).toBeInTheDocument();
  });
});
