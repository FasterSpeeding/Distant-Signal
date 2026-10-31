import { beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { KINGS_CROSS, RUSSELL_SQUARE, RUSSELL_SQUARE_ARRIVALS, tflLineStatus } from '@/test/tflStopFixtures';
import { TFL_NETWORK } from '@/test/transitFixtures';
import * as api from '@/lib/api';
import { ApiNotFoundError, ApiUnavailableError } from '@/lib/api';
import TflStopPage, { generateMetadata } from './page';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getTflStop: vi.fn(),
    getTflStopArrivals: vi.fn(),
    getLineStatus: vi.fn(),
    getTransitNetworks: vi.fn(),
    getTransitStop: vi.fn(),
    getTransitDepartures: vi.fn(),
  };
});

const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
}));

async function render(id = '940GZZLURSQ', search: Record<string, string> = {}) {
  const element = await TflStopPage({ params: Promise.resolve({ id }), searchParams: Promise.resolve(search) });
  return renderWithMantine(element);
}

describe('/tfl/stops/[id]', () => {
  beforeEach(() => {
    cleanup();
    notFoundMock.mockClear();
    vi.mocked(api.getTflStop).mockReset().mockResolvedValue(RUSSELL_SQUARE);
    vi.mocked(api.getTflStopArrivals).mockReset().mockResolvedValue(RUSSELL_SQUARE_ARRIVALS);
    vi.mocked(api.getLineStatus)
      .mockReset()
      .mockResolvedValue([tflLineStatus('piccadilly', 'Piccadilly', 9, 'Minor delays due to a train cancellation')]);
    // The TfL network is not served unless a test says so.
    vi.mocked(api.getTransitNetworks).mockReset().mockResolvedValue([]);
    vi.mocked(api.getTransitStop).mockReset();
    vi.mocked(api.getTransitDepartures).mockReset();
  });

  it('has no timetable while the TfL network is not served', async () => {
    await render();
    expect(screen.queryByRole('heading', { name: 'Timetable' })).not.toBeInTheDocument();
    expect(api.getTransitStop).not.toHaveBeenCalled();
  });

  it("shows the stop's first and last trains and its scheduled departures", async () => {
    vi.mocked(api.getTransitNetworks).mockResolvedValue([TFL_NETWORK]);
    vi.mocked(api.getTransitStop).mockResolvedValue({
      id: '9400ZZLURSQ',
      name: 'Russell Square',
      lat: null,
      lon: null,
      lines: ['tfl:piccadilly'],
      railLinks: [],
      platforms: [],
      firstLast: [
        {
          line: { id: 'tfl:piccadilly', lineId: 'piccadilly', name: 'Piccadilly', colour: null },
          direction: 'outbound',
          towards: 'Cockfosters',
          first: '05:48',
          last: '00:31',
        },
      ],
    });
    vi.mocked(api.getTransitDepartures).mockResolvedValue({
      date: '2026-10-30',
      departures: [
        {
          scheduledDeparture: '2026-10-30T23:40:00+00:00',
          tripId: 'T1',
          platformId: '9400ZZLURSQ1',
          platform: null,
          line: { id: 'tfl:piccadilly', lineId: 'piccadilly', name: 'Piccadilly', colour: null },
          replacementBus: false,
          destination: 'Cockfosters',
        },
      ],
      nextCursor: '85200.T1.9400ZZLURSQ1',
      outsideHorizon: false,
      realtime: false,
      kind: 'scheduled',
    });
    await render('940GZZLURSQ', { date: '2026-10-30' });
    expect(api.getTransitStop).toHaveBeenCalledWith('tfl', '940GZZLURSQ', '2026-10-30');
    expect(screen.getByRole('heading', { level: 3, name: /^First and last on / })).toBeInTheDocument();
    expect(api.getTransitDepartures).toHaveBeenCalledWith('tfl', '940GZZLURSQ', {
      date: '2026-10-30',
      cursor: undefined,
      limit: '10',
    });
    expect(screen.getByRole('heading', { level: 2, name: 'Timetable' })).toBeInTheDocument();
    expect(screen.getByText('Piccadilly towards Cockfosters')).toBeInTheDocument();
    expect(screen.getByText('First 05:48, last 00:31')).toBeInTheDocument();
    expect(screen.getByRole('list', { name: 'Scheduled departures' })).toHaveTextContent('23:40');
    expect(screen.getByRole('link', { name: 'Later departures' })).toHaveAttribute(
      'href',
      '/tfl/stops/940GZZLURSQ?date=2026-10-30&cursor=85200.T1.9400ZZLURSQ1#timetable',
    );
    expect(screen.getByRole('link', { name: 'Next day' })).toHaveAttribute(
      'href',
      '/tfl/stops/940GZZLURSQ?date=2026-10-31#timetable',
    );
  });

  it('has no timetable for a stop the TfL timetable lacks', async () => {
    vi.mocked(api.getTransitNetworks).mockResolvedValue([TFL_NETWORK]);
    vi.mocked(api.getTransitStop).mockRejectedValue(new ApiNotFoundError('no such stop'));
    vi.mocked(api.getTransitDepartures).mockRejectedValue(new ApiNotFoundError('no such stop'));
    await render();
    expect(screen.queryByRole('heading', { name: 'Timetable' })).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: 'Live arrivals' })).toBeInTheDocument();
  });

  it('shows the stop, its mode, its live arrivals and when they were fetched', async () => {
    await render();
    expect(screen.getByRole('heading', { level: 1, name: 'Russell Square' })).toBeInTheDocument();
    expect(screen.getByText('Underground')).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: 'Live arrivals' })).toBeInTheDocument();
    expect(screen.getByRole('list', { name: 'Piccadilly, Eastbound - Platform 1' })).toBeInTheDocument();
    expect(screen.getByText(/^Updated/)).toBeInTheDocument();
    expect(api.getTflStopArrivals).toHaveBeenCalledWith('940GZZLURSQ');
    // TfL's credit is the site footer's (on every page), not the page's own.
    expect(screen.queryByText(/Powered by TfL Open Data/)).not.toBeInTheDocument();
  });

  it("shows its lines' status, linking each line's page", async () => {
    await render();
    expect(api.getLineStatus).toHaveBeenCalledWith(['tfl-piccadilly'], false);
    expect(screen.getByRole('heading', { level: 2, name: 'Line status' })).toBeInTheDocument();
    expect(screen.getByText('Minor delays due to a train cancellation')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Piccadilly/ })).toHaveAttribute('href', '/lines/tfl-piccadilly');
  });

  it('links a line with no status yet, and every line when status fails', async () => {
    vi.mocked(api.getTflStop).mockResolvedValue(KINGS_CROSS);
    vi.mocked(api.getLineStatus).mockResolvedValue([tflLineStatus('northern', 'Northern')]);
    await render('940GZZLUKSX');
    expect(screen.getByText(/Also here/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Victoria' })).toHaveAttribute('href', '/lines/tfl-victoria');

    cleanup();
    vi.mocked(api.getLineStatus).mockRejectedValue(new ApiNotFoundError('none'));
    await render('940GZZLUKSX');
    expect(screen.getByText(/Lines here/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Northern' })).toHaveAttribute('href', '/lines/tfl-northern');
  });

  it('links the National Rail station at the stop, and offers to start a journey leg from it', async () => {
    vi.mocked(api.getTflStop).mockResolvedValue(KINGS_CROSS);
    await render('940GZZLUKSX');
    expect(screen.getByText(/National Rail stations here/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'London Kings Cross (KGX)' })).toHaveAttribute('href', '/stations/KGX');
    expect(screen.getByRole('link', { name: 'London St Pancras International (STP)' })).toHaveAttribute(
      'href',
      '/stations/STP',
    );
    expect(screen.getByRole('link', { name: 'Add as a journey leg' })).toHaveAttribute(
      'href',
      '/plan?fromTflStop=940GZZLUKSX',
    );

    cleanup();
    vi.mocked(api.getTflStop).mockResolvedValue(RUSSELL_SQUARE);
    await render();
    expect(screen.queryByText(/National Rail station here/)).not.toBeInTheDocument();
  });

  it('keeps the page when TfL is down, saying the arrivals could not be loaded', async () => {
    vi.mocked(api.getTflStopArrivals).mockRejectedValue(new ApiUnavailableError('503'));
    await render();
    expect(screen.getByRole('heading', { level: 1, name: 'Russell Square' })).toBeInTheDocument();
    expect(screen.getByText("Couldn't load live arrivals")).toBeInTheDocument();
    expect(screen.getByText(/tries again every 30 seconds/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: 'Line status' })).toBeInTheDocument();
  });

  it('is a 404 for an unknown or malformed stop, without asking for one that cannot exist', async () => {
    vi.mocked(api.getTflStop).mockRejectedValue(new ApiNotFoundError('404'));
    await expect(render('940GZZLUNOPE')).rejects.toThrow('NEXT_NOT_FOUND');

    vi.mocked(api.getTflStop).mockClear();
    await expect(render('..%2Fmetrics')).rejects.toThrow('NEXT_NOT_FOUND');
    expect(api.getTflStop).not.toHaveBeenCalled();
  });

  it('titles the page with the stop name', async () => {
    const metadata = await generateMetadata({ params: Promise.resolve({ id: '940GZZLURSQ' }) });
    expect(metadata.title).toBe('Russell Square');
    expect(metadata.description).toBe('Live arrivals and line status at Russell Square (Underground).');
  });
});
