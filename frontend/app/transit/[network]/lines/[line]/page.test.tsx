import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LINE, NETWORK } from '@/test/transitFixtures';
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

  it('lists the stops in order each way and links the timetable', async () => {
    renderWithMantine(await TransitLinePage(params('blue')));
    expect(screen.getByRole('heading', { level: 1, name: 'Blue Line' })).toBeTruthy();
    const towardsEccles = screen.getByRole('heading', { name: 'Towards Eccles' }).parentElement!;
    const stops = within(towardsEccles)
      .getAllByRole('link')
      .map((a) => a.textContent);
    expect(stops).toEqual(['Piccadilly', 'Eccles']);
    expect(screen.getByRole('heading', { name: 'Towards Piccadilly' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Timetable' }).getAttribute('href')).toBe(
      '/transit/metrolink/lines/blue/timetable',
    );
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
