import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import {
  DELAY,
  LINE,
  MINOR_DELAYS,
  NETWORK,
  NOTHING_REPORTED,
  SITUATIONS,
  TFL_BRANCHED_LINE,
  TFL_LINE,
  TFL_NETWORK,
} from '@/test/transitFixtures';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import TransitLinePage, { generateMetadata } from './page';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getTransitNetwork: vi.fn(), getTransitLine: vi.fn(), getPreferences: vi.fn() };
});

const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  notFound: () => notFoundMock(),
}));

const params = (line: string) => ({ params: Promise.resolve({ network: 'metrolink', line }) });

describe('/transit/[network]/lines/[line]', () => {
  beforeEach(() => {
    cleanup();
    vi.mocked(api.getTransitNetwork).mockResolvedValue(NETWORK);
    vi.mocked(api.getTransitLine).mockResolvedValue(LINE);
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
  });

  it('draws the stops once, in order, with the timetable each way', async () => {
    renderWithMantine(await TransitLinePage(params('blue')));
    expect(screen.getByRole('heading', { level: 1, name: 'Blue Line' })).toBeTruthy();
    // One list, not one per direction.
    const stops = screen.getByRole('list', { name: 'Blue Line stops, Piccadilly to Eccles' });
    expect(
      within(stops)
        .getAllByRole('link')
        .map((a) => a.textContent),
    ).toEqual(['Piccadilly', 'Eccles']);
    expect(screen.queryByRole('heading', { name: 'Towards Eccles' })).toBeNull();
    expect(screen.getByText(/Piccadilly to Eccles, both ways/)).toBeTruthy();
    const each = screen.getByRole('list', { name: 'Timetable each way' });
    expect(
      within(each)
        .getAllByRole('link')
        .map((a) => [a.textContent, a.getAttribute('href')]),
    ).toEqual([
      ['Timetable: towards Eccles', '/transit/metrolink/lines/blue/timetable?direction=outbound'],
      ['Timetable: towards Piccadilly', '/transit/metrolink/lines/blue/timetable?direction=inbound'],
    ]);
    expect(screen.getByRole('link', { name: 'Timetable' }).getAttribute('href')).toBe(
      '/transit/metrolink/lines/blue/timetable',
    );
  });

  it("shows a TfL line's TfL status, how often it runs, and no disruptions list", async () => {
    vi.mocked(api.getTransitNetwork).mockResolvedValue(TFL_NETWORK);
    vi.mocked(api.getTransitLine).mockResolvedValue(TFL_LINE);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: ['tfl-victoria'],
      pinnedStations: [],
      pinnedOperators: [],
    });
    renderWithMantine(await TransitLinePage({ params: Promise.resolve({ network: 'tfl', line: 'victoria' }) }));
    expect(screen.getByText('Minor delays')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Status details' }).getAttribute('href')).toBe('/lines/tfl-victoria');
    expect(screen.queryByRole('heading', { name: 'Disruptions' })).toBeNull();
    // No headsign: the direction is named by its last stop.
    expect(screen.getByRole('heading', { name: 'Towards Walthamstow Central' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Timetable: towards Walthamstow Central' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'How often it runs today' })).toBeTruthy();
    expect(screen.getByText('From Brixton: first 05:27, last 00:28')).toBeTruthy();
    expect(screen.getByText('Every 1 to 3 minutes')).toBeTruthy();
    // The pin is TfL's line, already pinned.
    expect(screen.getByRole('button', { name: /unpin/i })).toBeTruthy();
  });

  it('lists a branched direction by branch, each with its stops and timetable', async () => {
    vi.mocked(api.getTransitNetwork).mockResolvedValue(TFL_NETWORK);
    vi.mocked(api.getTransitLine).mockResolvedValue(TFL_BRANCHED_LINE);
    renderWithMantine(await TransitLinePage({ params: Promise.resolve({ network: 'tfl', line: 'northern' }) }));
    // The busiest branch is the main line; the other hangs off Morden.
    const stops = screen.getByRole('list', { name: 'Northern stops, Morden to Edgware' });
    const branch = within(stops).getByRole('group', { name: 'Branch to High Barnet' });
    expect(within(branch).getByRole('link', { name: 'High Barnet' })).toBeTruthy();
    expect(screen.getAllByRole('link', { name: 'Morden' })).toHaveLength(1);
    expect(screen.getByRole('link', { name: 'Timetable: towards Edgware or High Barnet' })).toBeTruthy();
    // A TfL station links TfL's stop page.
    expect(screen.getByRole('link', { name: 'Morden' }).getAttribute('href')).toBe('/tfl/stops/940GZZLUMDN');
    // Every branch's timetable, in a closed panel.
    expect(screen.getByRole('button', { name: /Timetables by branch/ })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Morden to High Barnet', hidden: true }).getAttribute('href')).toBe(
      '/transit/tfl/lines/northern/timetable?direction=outbound&branch=outbound-2',
    );
  });

  it("shows the line's status and its disruptions", async () => {
    vi.mocked(api.getTransitLine).mockResolvedValue({
      ...LINE,
      status: MINOR_DELAYS,
      situations: { ...SITUATIONS, current: [DELAY], upcoming: [] },
    });
    renderWithMantine(await TransitLinePage(params('blue')));
    expect(screen.getByText('Minor delays')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Disruptions' })).toBeTruthy();
    expect(screen.getByText(DELAY.summary)).toBeTruthy();
    cleanup();
    vi.mocked(api.getTransitLine).mockResolvedValue({
      ...LINE,
      status: NOTHING_REPORTED,
      situations: { ...SITUATIONS, current: [], upcoming: [] },
    });
    renderWithMantine(await TransitLinePage(params('blue')));
    expect(screen.getByText('No reported disruption')).toBeTruthy();
    expect(screen.getByText('Nothing is affecting this line right now.')).toBeTruthy();
    cleanup();
    // No feed: neither status nor a disruptions section.
    vi.mocked(api.getTransitLine).mockResolvedValue(LINE);
    renderWithMantine(await TransitLinePage(params('blue')));
    expect(screen.queryByRole('heading', { name: 'Disruptions' })).toBeNull();
  });

  it('offers the follow star, pinned when the public line id is pinned', async () => {
    renderWithMantine(await TransitLinePage(params('blue')));
    expect(screen.getByRole('button', { name: 'Pin (currently not pinned)' })).toBeTruthy();
    cleanup();
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: ['metrolink:blue'],
      pinnedStations: [],
      pinnedOperators: [],
    });
    renderWithMantine(await TransitLinePage(params('blue')));
    expect(screen.getByRole('button', { name: 'Unpin (currently pinned)' })).toBeTruthy();
    cleanup();
    // No session or an outage: unpinned, never an error.
    vi.mocked(api.getPreferences).mockRejectedValue(new Error('500'));
    renderWithMantine(await TransitLinePage(params('blue')));
    expect(screen.getByRole('button', { name: 'Pin (currently not pinned)' })).toBeTruthy();
  });

  it('is not found for a public id in the path or an unknown line', async () => {
    await expect(TransitLinePage(params('metrolink:blue'))).rejects.toThrow('NEXT_NOT_FOUND');
    vi.mocked(api.getTransitLine).mockRejectedValue(new ApiNotFoundError('404'));
    await expect(TransitLinePage(params('nope'))).rejects.toThrow('NEXT_NOT_FOUND');
  });

  it('titles the page with the line and network', async () => {
    expect((await generateMetadata(params('blue'))).title).toBe('Blue Line, Manchester Metrolink');
  });
});
