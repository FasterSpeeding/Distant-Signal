import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { BOARD, LIFT, NETWORK, STOP } from '@/test/transitFixtures';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import TransitStopPage from './page';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getTransitNetwork: vi.fn(),
    getTransitNetworks: vi.fn(),
    getTransitStop: vi.fn(),
    getTransitDepartures: vi.fn(),
  };
});

const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
const redirectMock = vi.fn((to: string) => {
  throw new Error(`NEXT_REDIRECT ${to}`);
});
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
  permanentRedirect: (to: string) => redirectMock(to),
}));

async function render(search: Record<string, string> = {}, stop = '9400ZZMAPIC') {
  const element = await TransitStopPage({
    params: Promise.resolve({ network: 'metrolink', stop }),
    searchParams: Promise.resolve(search),
  });
  return renderWithMantine(element);
}

describe('/transit/tfl/stops/[stop]', () => {
  it("redirects to TfL's stop page, keeping the day and page", async () => {
    await expect(
      TransitStopPage({
        params: Promise.resolve({ network: 'tfl', stop: '9400ZZLUKSX' }),
        searchParams: Promise.resolve({ date: '2026-10-30', cursor: 'c1' }),
      }),
    ).rejects.toThrow('NEXT_REDIRECT /tfl/stops/940GZZLUKSX?date=2026-10-30&cursor=c1#timetable');
    await expect(TransitStopPage({ params: Promise.resolve({ network: 'tfl', stop: '940GZZLUKSX' }) })).rejects.toThrow(
      /^NEXT_REDIRECT \/tfl\/stops\/940GZZLUKSX$/,
    );
  });
});

describe('/transit/[network]/stops/[stop]', () => {
  beforeEach(() => {
    cleanup();
    vi.mocked(api.getTransitNetwork).mockResolvedValue(NETWORK);
    vi.mocked(api.getTransitNetworks).mockResolvedValue([NETWORK]);
    vi.mocked(api.getTransitStop).mockResolvedValue(STOP);
    vi.mocked(api.getTransitDepartures).mockResolvedValue(BOARD);
  });

  it('shows the scheduled board, labelled as not live, with no status or delay', async () => {
    await render();
    expect(screen.getByRole('heading', { level: 1, name: 'Piccadilly' })).toBeTruthy();
    expect(screen.getByText('Scheduled times, not live')).toBeTruthy();
    const rows = within(screen.getByRole('list', { name: 'Scheduled departures' })).getAllByRole('listitem');
    expect(rows).toHaveLength(2);
    expect(rows[0]!.textContent).toContain('07:02');
    expect(rows[0]!.textContent).toContain('Eccles');
    expect(rows[0]!.textContent).toContain('Blue Line · To Ashton');
    for (const row of rows) {
      expect(row.textContent).not.toMatch(/late|early|on time|cancelled|expected/i);
    }
    // A replacement bus says so.
    expect(rows[1]!.textContent).toContain('Replacement bus');
    // The lines calling here link to their pages.
    expect(screen.getByRole('link', { name: 'Green Line' }).getAttribute('href')).toBe(
      '/transit/metrolink/lines/green',
    );
  });

  it('links the National Rail station to change to', async () => {
    vi.mocked(api.getTransitStop).mockResolvedValue({
      ...STOP,
      railLinks: [
        { crs: 'MAN', tiploc: 'MNCRPIC', kind: 'interchange', note: null, stationName: 'Manchester Piccadilly' },
      ],
    });
    await render();
    expect(screen.getByText(/Change for National Rail/).textContent).toBe(
      'Change for National Rail at Manchester Piccadilly',
    );
    expect(screen.getByRole('link', { name: 'Manchester Piccadilly' }).getAttribute('href')).toBe('/stations/MAN');
    cleanup();
    vi.mocked(api.getTransitStop).mockResolvedValue({ ...STOP, railLinks: [] });
    await render();
    expect(screen.queryByText(/National Rail/)).toBeNull();
  });

  it("shows the stop's notices above the board, and nothing without any", async () => {
    vi.mocked(api.getTransitStop).mockResolvedValue({ ...STOP, notices: [LIFT] });
    await render();
    expect(screen.getByRole('heading', { name: 'Notices' })).toBeTruthy();
    expect(screen.getByText(LIFT.summary)).toBeTruthy();
    cleanup();
    vi.mocked(api.getTransitStop).mockResolvedValue({ ...STOP, notices: [] });
    await render();
    expect(screen.queryByRole('heading', { name: 'Notices' })).toBeNull();
  });

  it('pages with the cursor and offers the next day', async () => {
    await render();
    expect(screen.getByRole('link', { name: 'Later departures' }).getAttribute('href')).toBe(
      '/transit/metrolink/stops/9400ZZMAPIC?date=2026-10-14&cursor=25800.B1.9400ZZMAPIC1',
    );
    expect(screen.getByRole('link', { name: 'Next day' }).getAttribute('href')).toBe(
      '/transit/metrolink/stops/9400ZZMAPIC?date=2026-10-15',
    );
    cleanup();
    await render({ date: '2026-10-14', cursor: '25800.B1.9400ZZMAPIC1' });
    expect(api.getTransitDepartures).toHaveBeenLastCalledWith('metrolink', '9400ZZMAPIC', {
      date: '2026-10-14',
      cursor: '25800.B1.9400ZZMAPIC1',
    });
  });

  it('says when the board is empty or the date is outside the timetable', async () => {
    vi.mocked(api.getTransitDepartures).mockResolvedValue({ ...BOARD, departures: [], nextCursor: null });
    await render();
    expect(screen.getByText('No more scheduled departures this day.')).toBeTruthy();
    expect(screen.queryByRole('link', { name: 'Later departures' })).toBeNull();
    cleanup();
    vi.mocked(api.getTransitDepartures).mockResolvedValue({
      ...BOARD,
      departures: [],
      nextCursor: null,
      outsideHorizon: true,
    });
    await render();
    expect(screen.getByText("The timetable for this date isn't published yet.")).toBeTruthy();
  });

  it('is not found for an unknown or malformed stop', async () => {
    await expect(render({}, 'piccadilly')).rejects.toThrow('NEXT_NOT_FOUND');
    vi.mocked(api.getTransitStop).mockRejectedValue(new ApiNotFoundError('404'));
    await expect(render()).rejects.toThrow('NEXT_NOT_FOUND');
  });

  it('offers to plan a journey from the stop only when the planner routes over its network', async () => {
    await render();
    expect(screen.queryByRole('link', { name: 'Plan a journey from here' })).toBeNull();
    cleanup();
    vi.mocked(api.getTransitNetworks).mockResolvedValue([{ ...NETWORK, tripPlanning: true }]);
    await render();
    expect(screen.getByRole('link', { name: 'Plan a journey from here' }).getAttribute('href')).toBe(
      '/plan?origin=9400ZZMAPIC',
    );
  });
});
