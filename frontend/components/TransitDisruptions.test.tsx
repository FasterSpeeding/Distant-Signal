import { describe, it, expect, beforeEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import {
  DELAY,
  LIFT,
  MINOR_DELAYS,
  NOTHING_REPORTED,
  SITUATIONS,
  STATUS_UNAVAILABLE,
  WORKS,
} from '@/test/transitFixtures';
import { TransitDisruptions, TransitStopNotices } from './TransitDisruptions';
import { TransitStatusBadge } from './TransitStatusBadge';

const LINE_NAMES = { 'metrolink:blue': 'Blue Line', 'metrolink:green': 'Green Line' };

describe('TransitStatusBadge', () => {
  beforeEach(cleanup);

  it('shows the headline status in sentence case', () => {
    renderWithMantine(<TransitStatusBadge status={MINOR_DELAYS} />);
    expect(screen.getByText('Minor delays')).toBeTruthy();
    cleanup();
    renderWithMantine(<TransitStatusBadge status={NOTHING_REPORTED} />);
    expect(screen.getByText('No reported disruption')).toBeTruthy();
    expect(screen.queryByText(/good service/i)).toBeNull();
  });

  it('says the status is unavailable once the disruptions are stale', () => {
    renderWithMantine(<TransitStatusBadge status={STATUS_UNAVAILABLE} />);
    expect(screen.getByText('Status unavailable')).toBeTruthy();
  });

  it('shows nothing for a network without a disruptions feed', () => {
    renderWithMantine(<TransitStatusBadge status={{ ...STATUS_UNAVAILABLE, state: 'none', asOf: null }} />);
    expect(document.querySelector('.mantine-Badge-root')).toBeNull();
    cleanup();
    renderWithMantine(<TransitStatusBadge status={undefined} />);
    expect(document.querySelector('.mantine-Badge-root')).toBeNull();
  });
});

describe('TransitDisruptions', () => {
  beforeEach(cleanup);

  it('lists the current disruptions, then those coming up', () => {
    renderWithMantine(<TransitDisruptions situations={SITUATIONS} empty="None." lineNames={LINE_NAMES} />);
    expect(screen.getByRole('heading', { name: 'Disruptions' })).toBeTruthy();
    const now = screen.getByRole('list', { name: 'Disruptions now' });
    const items = within(now).getAllByRole('listitem');
    expect(items).toHaveLength(2);
    expect(items[0]!.textContent).toContain(DELAY.summary);
    expect(items[0]!.textContent).toContain('Blue Line');
    expect(items[0]!.textContent).toContain('Allow extra time.');
    expect(items[0]!.textContent).toMatch(/^.*Since \d{1,2} Oct 2026, \d{2}:\d{2}/);
    expect(items[1]!.textContent).toMatch(/Until \d{1,2} Oct 2099/);
    const soon = screen.getByRole('list', { name: 'Disruptions coming up' });
    expect(soon.textContent).toContain(WORKS.summary);
    expect(soon.textContent).toContain('Green Line');
    expect(soon.textContent).toMatch(/From 18 Oct 2026, 06:00 to 20 Oct 2026, 00:00/);
    expect(screen.getByText('Coming up')).toBeTruthy();
  });

  it('renders a description as text, never as markup', () => {
    const { container } = renderWithMantine(<TransitDisruptions situations={SITUATIONS} empty="None." />);
    expect(screen.getByText('A <b>fault</b> at Cornbrook.')).toBeTruthy();
    expect(container.querySelector('b')).toBeNull();
  });

  it('links only a web address', () => {
    renderWithMantine(<TransitDisruptions situations={SITUATIONS} empty="None." />);
    const links = screen.getAllByRole('link', { name: /More details/ });
    expect(links).toHaveLength(1);
    expect(links[0]!.getAttribute('href')).toBe('https://example.invalid/s1');
  });

  it('links each stop a disruption names to its stop page', () => {
    renderWithMantine(
      <TransitDisruptions
        situations={SITUATIONS}
        empty="None."
        network="metrolink"
        stopNames={{ '9400ZZMAPIC': 'Piccadilly' }}
      />,
    );
    const lift = within(screen.getByRole('list', { name: 'Disruptions now' }))
      .getAllByRole('listitem')
      .find((item) => item.textContent.includes(LIFT.summary))!;
    expect(lift.textContent).toContain('At Piccadilly');
    expect(within(lift).getByRole('link', { name: 'Piccadilly' }).getAttribute('href')).toBe(
      '/transit/metrolink/stops/9400ZZMAPIC',
    );
    // A line disruption names no stops, so links none (bar "More details").
    const delay = within(screen.getByRole('list', { name: 'Disruptions now' }))
      .getAllByRole('listitem')
      .find((item) => item.textContent.includes(DELAY.summary))!;
    expect(
      within(delay)
        .getAllByRole('link')
        .map((a) => a.textContent),
    ).toEqual(['More details']);
  });

  it('lists several stops, and leaves out a stop it has no name for', () => {
    const works = { ...LIFT, id: 'S9', stops: ['9400ZZMAPIC', '9400ZZMAGMX', '9400ZZMAECC', '9400ZZMAXXX'] };
    renderWithMantine(
      <TransitDisruptions
        situations={{ ...SITUATIONS, current: [works], upcoming: [] }}
        empty="None."
        network="metrolink"
        stopNames={{ '9400ZZMAPIC': 'Piccadilly', '9400ZZMAGMX': 'Deansgate-Castlefield', '9400ZZMAECC': 'Eccles' }}
      />,
    );
    expect(screen.getByText(/^At /).textContent).toBe('At Piccadilly, Deansgate-Castlefield and Eccles');
    expect(screen.getByRole('link', { name: 'Eccles' }).getAttribute('href')).toBe(
      '/transit/metrolink/stops/9400ZZMAECC',
    );
  });

  it('links no stops without a network, as on a stop page', () => {
    renderWithMantine(<TransitDisruptions situations={SITUATIONS} empty="None." />);
    expect(screen.queryByRole('link', { name: 'Piccadilly' })).toBeNull();
    expect(screen.queryByText(/^At /)).toBeNull();
  });

  it('says when nothing is reported', () => {
    renderWithMantine(
      <TransitDisruptions
        situations={{ ...SITUATIONS, current: [], upcoming: [] }}
        empty="Nothing is affecting this line right now."
      />,
    );
    expect(screen.getByText('Nothing is affecting this line right now.')).toBeTruthy();
    expect(screen.queryByText('Coming up')).toBeNull();
  });

  it('says the status is unavailable, with when it was last read, and lists nothing', () => {
    renderWithMantine(
      <TransitDisruptions
        situations={{ ...SITUATIONS, state: 'unavailable', asOf: '2026-10-14T08:00:00Z' }}
        empty="None."
      />,
    );
    expect(screen.getByText('Status unavailable. Last read at 09:00.')).toBeTruthy();
    expect(screen.queryByText(DELAY.summary)).toBeNull();
  });

  it('shows nothing without a feed, and a failure when loading them failed', () => {
    renderWithMantine(<TransitDisruptions situations={{ ...SITUATIONS, state: 'none' }} empty="None." />);
    expect(screen.queryByRole('heading', { name: 'Disruptions' })).toBeNull();
    expect(screen.queryByText(DELAY.summary)).toBeNull();
    cleanup();
    renderWithMantine(<TransitDisruptions situations={null} failed empty="None." />);
    expect(screen.getByText(/^Couldn't load disruptions\./)).toBeTruthy();
  });
});

describe('TransitStopNotices', () => {
  beforeEach(cleanup);

  it("lists a stop's notices, and nothing when there are none", () => {
    renderWithMantine(<TransitStopNotices notices={[LIFT]} />);
    expect(screen.getByRole('heading', { name: 'Notices' })).toBeTruthy();
    expect(within(screen.getByRole('list', { name: 'Notices' })).getByText(LIFT.summary)).toBeTruthy();
    cleanup();
    renderWithMantine(<TransitStopNotices notices={[]} />);
    expect(screen.queryByRole('heading', { name: 'Notices' })).toBeNull();
  });
});
