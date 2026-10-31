import { describe, it, expect, beforeEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { MINOR_DELAYS, NETWORK, NOTHING_REPORTED, SITUATIONS } from '@/test/transitFixtures';
import type { TransitLineStatus } from '@/lib/types';
import {
  disruptionCount,
  lineDisruptions,
  TransitDisruptionSummary,
  TransitLineDisruption,
} from './TransitDisruptionSummary';

const BLUE = NETWORK.lines[0]!;

/** Blue with two disruptions, the worst first. */
const TWO: TransitLineStatus = {
  ...MINOR_DELAYS,
  statuses: [
    { severity: 6, description: 'Severe Delays', reason: 'Points fault', situationId: 'S7' },
    ...MINOR_DELAYS.statuses,
  ],
};

describe('lineDisruptions and disruptionCount', () => {
  it("takes a line's worst disruption and counts the rest", () => {
    expect(lineDisruptions({ ...BLUE, status: TWO })).toMatchObject({ headline: { situationId: 'S7' }, more: 1 });
    expect(lineDisruptions({ ...BLUE, status: NOTHING_REPORTED })).toBeNull();
    expect(lineDisruptions({ ...BLUE, status: { ...TWO, state: 'unavailable' } })).toBeNull();
    expect(lineDisruptions(BLUE)).toBeNull();
  });

  it('says how many in one sentence', () => {
    expect(disruptionCount(0, 0)).toBe('No disruptions reported now.');
    expect(disruptionCount(1, 0)).toBe('1 disruption now.');
    expect(disruptionCount(3, 2)).toBe('3 disruptions now, 2 coming up.');
  });
});

describe('TransitLineDisruption', () => {
  beforeEach(cleanup);

  it("links the headline to its page and the rest to the line's", () => {
    renderWithMantine(<TransitLineDisruption network="metrolink" line={{ ...BLUE, status: TWO }} />);
    expect(screen.getByText('Severe delays')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Points fault' }).getAttribute('href')).toBe(
      '/transit/metrolink/incidents/S7',
    );
    expect(screen.getByRole('link', { name: 'and 1 more' }).getAttribute('href')).toBe('/transit/metrolink/lines/blue');
  });

  it('leaves the chip out for a row that shows the status already, and renders nothing without one', () => {
    renderWithMantine(
      <TransitLineDisruption network="metrolink" line={{ ...BLUE, status: MINOR_DELAYS }} chip={false} />,
    );
    expect(screen.queryByText('Minor delays')).toBeNull();
    expect(screen.getByRole('link', { name: MINOR_DELAYS.statuses[0]!.reason })).toBeTruthy();
    cleanup();
    const { container } = renderWithMantine(
      <TransitLineDisruption network="metrolink" line={{ ...BLUE, status: NOTHING_REPORTED }} />,
    );
    expect(container.querySelector('[data-line-disruption]')).toBeNull();
  });
});

describe('TransitDisruptionSummary', () => {
  beforeEach(cleanup);

  it('shows nothing without a feed, and says when it is stale or failed', () => {
    renderWithMantine(
      <TransitDisruptionSummary
        network="metrolink"
        lines={NETWORK.lines}
        situations={{ state: 'none', asOf: null, current: [], upcoming: [] }}
      />,
    );
    expect(screen.queryByRole('heading', { name: 'Disruptions' })).toBeNull();
    cleanup();
    renderWithMantine(<TransitDisruptionSummary network="metrolink" lines={NETWORK.lines} situations={null} failed />);
    expect(screen.getByText(/^Couldn't load disruptions\./)).toBeTruthy();
  });

  it('lists each line once, with notices no line covers and what is coming up', () => {
    renderWithMantine(
      <TransitDisruptionSummary
        network="metrolink"
        lines={[
          { ...BLUE, status: MINOR_DELAYS },
          { ...NETWORK.lines[1]!, status: NOTHING_REPORTED },
        ]}
        situations={SITUATIONS}
      />,
    );
    expect(within(screen.getByRole('list', { name: 'Disruptions by line' })).getAllByRole('listitem')).toHaveLength(1);
    expect(within(screen.getByRole('list', { name: 'Other notices' })).getAllByRole('listitem')).toHaveLength(1);
    const upcoming = within(screen.getByRole('list', { name: 'Disruptions coming up' })).getByRole('listitem');
    expect(upcoming.textContent).toMatch(/^From 18 Oct 2026, 06:00 to 20 Oct 2026, 00:00: No trams/);
  });
});
