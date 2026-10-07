import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { visibleText } from '@/test/routeText';
import { LineTrainsResults } from './LineTrainsResults';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { LineTrainsSummary } from '@/lib/types';
import { parseLinePageParams } from '@/lib/lineTrains';
import { STATIONS, train } from '@/test/lineTrainsFixtures';

vi.mock('@/lib/api');

// 13:00Z on 2026-10-06 is 14:00 BST: the window is 13:30-16:00 (15:00 on a
// phone).
const NOW = new Date('2026-10-06T13:00:00Z');
const DATE = '2026-10-06';
const ID = 'swr-south-west-main';

function summary(overrides: Partial<LineTrainsSummary> = {}): LineTrainsSummary {
  return {
    lineId: ID,
    date: DATE,
    scopeApplied: true,
    scopes: ['line', 'shared'],
    window: { from: '13:30', to: '16:00' },
    at: '14:00',
    directions: null,
    stations: STATIONS,
    counts: { line: { up: 1, down: 2 } },
    truncated: false,
    trains: [],
    running: [],
    ...overrides,
  };
}

async function render(params: Record<string, string> = {}) {
  renderWithMantine(
    await LineTrainsResults({
      id: ID,
      date: DATE,
      now: NOW,
      params: parseLinePageParams(params),
      operatorName: (code) => ({ XC: 'CrossCountry', SW: 'South Western Railway' })[code] ?? code,
    }),
  );
}

/** The rows of the main list, as their visible text. */
function rowTexts(list: HTMLElement): string[] {
  return within(list)
    .getAllByRole('link')
    .map((link) => visibleText(link));
}

beforeEach(() => {
  vi.mocked(api.getLineTrainsSummary).mockReset();
  vi.mocked(api.searchTrainsBetween).mockReset();
});

describe('LineTrainsResults', () => {
  it('renders the "not available" state on a 404 and the outage state otherwise', async () => {
    vi.mocked(api.getLineTrainsSummary).mockRejectedValue(new ApiNotFoundError('not found'));
    await render();
    expect(screen.getByText('No scheduled train data is available for this line today.')).toBeInTheDocument();

    vi.mocked(api.getLineTrainsSummary).mockRejectedValue(new Error('connect ECONNREFUSED'));
    await render();
    expect(screen.getByText("Today's trains aren't available right now.")).toBeInTheDocument();
  });

  it('asks for the window around now, with at=now for Running now', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(summary());
    await render();
    expect(api.getLineTrainsSummary).toHaveBeenCalledWith(ID, {
      date: DATE,
      from: '13:30',
      to: '16:00',
      at: '14:00',
      direction: undefined,
    });
  });

  it('lists the line’s own trains by time on the line, with destination and status, each row a link', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(
      summary({
        trains: [
          train({
            uid: 'B',
            lineDue: { time: '14:35', dayOffset: 0 },
            live: {
              status: 'en_route',
              delayMinutes: 4,
              delayProvisional: false,
              cancelled: false,
              lastReportedLocation: null,
            },
          }),
          train({
            uid: 'A',
            direction: 'up',
            lineDue: { time: '14:05', dayOffset: 0 },
            destination: { crs: 'WAT', name: 'London Waterloo' },
          }),
          train({
            uid: 'C',
            lineDue: { time: '15:20', dayOffset: 0 },
            live: {
              status: 'cancelled',
              delayMinutes: null,
              delayProvisional: false,
              cancelled: true,
              lastReportedLocation: null,
            },
          }),
        ],
      }),
    );
    await render();
    const list = screen.getByRole('list', { name: 'Trains due on the line' });
    const texts = rowTexts(list);
    expect(texts[0]).toMatch(/^14:05 London Waterloo Scheduled/);
    expect(texts[1]).toMatch(/^14:35 Weymouth 4 min late/);
    expect(texts[2]).toMatch(/^15:20 Weymouth Cancelled/);
    expect(within(list).getAllByRole('link')[0]).toHaveAttribute('href', `/train/A/${DATE}`);
    expect(screen.getByText(/Due on the line 13:30–/)).toBeInTheDocument();
  });

  it('marks rows past the phone window so a phone shows an hour', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(
      summary({
        trains: [
          train({ uid: 'A', lineDue: { time: '14:50', dayOffset: 0 } }),
          train({ uid: 'B', lineDue: { time: '15:10', dayOffset: 0 } }),
        ],
      }),
    );
    const { container } = renderWithMantine(
      await LineTrainsResults({ id: ID, date: DATE, now: NOW, params: parseLinePageParams({}) }),
    );
    expect(container.querySelector('[data-uid="A"]')).not.toHaveAttribute('data-beyond-phone');
    expect(container.querySelector('[data-uid="B"]')).toHaveAttribute('data-beyond-phone', 'true');
  });

  it('has plain Earlier/Later links for desktop (2 h) and phone (1 h), keeping the direction', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(summary());
    await render({ dir: 'up' });
    const earlier = screen.getAllByRole('link', { name: 'Earlier trains' }).map((a) => a.getAttribute('href'));
    const later = screen.getAllByRole('link', { name: 'Later trains' }).map((a) => a.getAttribute('href'));
    expect(earlier).toEqual([`/lines/${ID}?dir=up&at=12%3A00#trains`, `/lines/${ID}?dir=up&at=13%3A00#trains`]);
    expect(later).toEqual([`/lines/${ID}?dir=up&at=16%3A00#trains`, `/lines/${ID}?dir=up&at=15%3A00#trains`]);
    expect(screen.queryByRole('link', { name: 'Trains due now' })).not.toBeInTheDocument();
  });

  it('pages from a chosen time without a Running now section, with a link back to now', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(summary({ running: null }));
    await render({ at: '17:00' });
    expect(api.getLineTrainsSummary).toHaveBeenCalledWith(
      ID,
      expect.objectContaining({ from: '16:30', to: '19:00', at: undefined }),
    );
    expect(screen.getByRole('heading', { name: /Due on the line from 17:00/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Trains due now' })).toHaveAttribute('href', `/lines/${ID}#trains`);
  });

  it('shows Running now once, not again in the window list', async () => {
    const running = train({ uid: 'R', lineDue: { time: '13:40', dayOffset: 0 } });
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(
      summary({
        trains: [running, train({ uid: 'N', lineDue: { time: '14:10', dayOffset: 0 } })],
        running: [train({ uid: 'E', lineDue: { time: '11:00', dayOffset: 0 } }), running],
      }),
    );
    const { container } = renderWithMantine(
      await LineTrainsResults({ id: ID, date: DATE, now: NOW, params: parseLinePageParams({}) }),
    );
    expect(screen.getByRole('heading', { name: 'Running now (2)' })).toBeInTheDocument();
    expect(container.querySelectorAll('[data-uid="R"]')).toHaveLength(1);
    expect(container.querySelectorAll('[data-uid="E"]')).toHaveLength(1);
    expect(container.querySelectorAll('[data-uid="N"]')).toHaveLength(1);
  });

  it('labels direction tabs by terminus with counts, marks the current one, and shows Loop only when needed', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(summary());
    await render({ dir: 'down' });
    const nav = screen.getByRole('navigation', { name: 'Direction' });
    const tabs = within(nav).getAllByRole('link');
    expect(tabs.map((t) => visibleText(t))).toEqual(['All 3', 'Towards London Waterloo 1', 'Towards Weymouth 2']);
    expect(within(nav).getByRole('link', { name: /Towards Weymouth/ })).toHaveAttribute('aria-current', 'page');
    expect(within(nav).getByRole('link', { name: /Towards London Waterloo/ })).toHaveAttribute(
      'href',
      `/lines/${ID}?dir=up#trains`,
    );
    expect(api.getLineTrainsSummary).toHaveBeenCalledWith(ID, expect.objectContaining({ direction: 'down' }));

    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(summary({ counts: { line: { loop: 4 } } }));
    await render();
    expect(screen.getAllByRole('link', { name: /^Loop/ }).length).toBeGreaterThan(0);
  });

  it('collapses shared trains into one group by operator and route', async () => {
    const xc = (uid: string, time: string) =>
      train({
        uid,
        scope: 'shared',
        operator: 'XC',
        lineDue: { time, dayOffset: 0 },
        origin: { crs: 'MAN', name: 'Manchester Piccadilly' },
        destination: { crs: 'BMH', name: 'Bournemouth' },
      });
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(
      summary({ trains: [train({ uid: 'OWN' }), xc('X1', '14:12'), xc('X2', '15:12')] }),
    );
    const { container } = renderWithMantine(
      await LineTrainsResults({
        id: ID,
        date: DATE,
        now: NOW,
        params: parseLinePageParams({}),
        operatorName: (c) => (c === 'XC' ? 'CrossCountry' : c),
      }),
    );
    const details = screen.getByText('Also running along part of this line (2)').closest('details');
    expect(details).not.toHaveAttribute('open');
    expect(
      within(details as HTMLElement).getByText('CrossCountry · Manchester Piccadilly → Bournemouth'),
    ).toBeInTheDocument();
    expect(
      within(details as HTMLElement).getByRole('link', { name: /14:12 CrossCountry train to Bournemouth/ }),
    ).toHaveAttribute('href', `/train/X1/${DATE}`);
    // Not in the main list.
    expect(container.querySelector('[data-uid="X1"]')).toBeNull();
  });

  it('links the hub stations’ timetables instead of listing trains that only touch the line', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(summary());
    await render();
    expect(screen.getByRole('link', { name: 'Other trains at London Waterloo →' })).toHaveAttribute(
      'href',
      '/stations/WAT#departures',
    );
    expect(screen.getByRole('link', { name: 'Other trains at Woking →' })).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /Other trains at Winchester/ })).not.toBeInTheDocument();
  });

  it('gives each row a text alternative for its stop strip and hides the visual strip from screen readers', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(
      summary({
        trains: [
          train({
            uid: 'A',
            lineDue: { time: '14:05', dayOffset: 0 },
            onLineStops: ['WAT', 'CLJ', 'WOK', 'WIN', 'SOU', 'WEY'].map((crs) => ({
              crs,
              time: '14:05',
              dayOffset: 0,
            })),
          }),
        ],
      }),
    );
    const { container } = renderWithMantine(
      await LineTrainsResults({ id: ID, date: DATE, now: NOW, params: parseLinePageParams({}) }),
    );
    const row = container.querySelector('[data-uid="A"] a') as HTMLElement;
    expect(row.textContent).toContain(
      'Then calls at Clapham Junction, Woking, Winchester, Southampton Central and Weymouth',
    );
    const strip = within(row).getByText(/Winchester · Southampton Central · Weymouth · \+2 stops/);
    expect(strip).toHaveAttribute('aria-hidden', 'true');
  });

  it('shows the shared mode badge when the API says the service is a replacement bus', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(
      summary({
        trains: [train({ uid: 'BUS', serviceMode: 'replacementBus', lineDue: { time: '14:30', dayOffset: 0 } })],
      }),
    );
    const { container } = renderWithMantine(
      await LineTrainsResults({ id: ID, date: DATE, now: NOW, params: parseLinePageParams({}) }),
    );
    const row = container.querySelector('[data-uid="BUS"] a') as HTMLElement;
    expect(within(row).getByText('Rail replacement bus')).toBeInTheDocument();
    expect(visibleText(row)).toMatch(/Timetable only/);
  });

  it('groups by route with a frequency summary in accessible accordions', async () => {
    const fast = (uid: string, time: string) =>
      train({
        uid,
        lineDue: { time, dayOffset: 0 },
        onLineStops: ['WAT', 'WOK', 'WEY'].map((crs) => ({ crs, time, dayOffset: 0 })),
      });
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(
      summary({
        trains: [
          fast('F1', '14:05'),
          fast('F2', '14:35'),
          fast('F3', '15:05'),
          train({
            uid: 'S1',
            lineDue: { time: '14:20', dayOffset: 0 },
            onLineStops: ['WAT', 'CLJ', 'WOK', 'BSK', 'WEY'].map((crs) => ({ crs, time: '14:20', dayOffset: 0 })),
          }),
        ],
      }),
    );
    await render({ view: 'routes' });
    const summaryEl = screen.getByText('London Waterloo → Weymouth, fast').closest('summary') as HTMLElement;
    expect(visibleText(summaryEl)).toBe(
      'London Waterloo → Weymouth, fast · every 30 min · xx:05, xx:35 · show 3 trains',
    );
    expect(summaryEl.closest('details')).not.toHaveAttribute('open');
    expect(screen.getByText('London Waterloo → Weymouth, stopping')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'By route' })).toHaveAttribute('aria-current', 'page');
  });

  it('lists trains between two picked stations from the station search, with live status where known', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(
      summary({
        trains: [
          train({
            uid: 'U1',
            live: {
              status: 'en_route',
              delayMinutes: 2,
              delayProvisional: false,
              cancelled: false,
              lastReportedLocation: null,
            },
          }),
        ],
      }),
    );
    vi.mocked(api.searchTrainsBetween).mockResolvedValue({
      results: [
        {
          uid: 'U1',
          scheduled: '14:12',
          publicDeparture: '14:12',
          stationCrs: 'WOK',
          originCrs: 'WEY',
          destinationCrs: 'WAT',
          destinationName: 'London Waterloo',
          destinationArrival: '14:40',
          destinationArrivalDayOffset: 0,
        },
        {
          uid: 'U2',
          scheduled: '14:20',
          publicDeparture: null,
          stationCrs: 'WOK',
          originCrs: 'POO',
          destinationCrs: 'WAT',
          destinationName: 'London Waterloo',
          destinationArrival: '14:52',
          destinationArrivalDayOffset: 0,
        },
      ],
      nextCursor: null,
    });
    const { container } = renderWithMantine(
      await LineTrainsResults({
        id: ID,
        date: DATE,
        now: NOW,
        params: parseLinePageParams({ from: 'WOK', to: 'WAT' }),
      }),
    );
    expect(api.searchTrainsBetween).toHaveBeenCalledWith({
      station: 'WOK',
      stopsAt: 'WAT',
      date: DATE,
      from: '13:30',
      to: '16:00',
      limit: 60,
    });
    expect(api.getLineTrainsSummary).toHaveBeenCalledWith(ID, expect.objectContaining({ direction: undefined }));
    expect(screen.getByRole('heading', { name: 'Woking to London Waterloo' })).toBeInTheDocument();
    expect(visibleText(container.querySelector('[data-uid="U1"] a') as HTMLElement)).toMatch(
      /^14:12 London Waterloo 2 min late/,
    );
    expect(visibleText(container.querySelector('[data-uid="U2"] a') as HTMLElement)).toMatch(
      /^14:20 London Waterloo Scheduled/,
    );
    expect(screen.getAllByRole('link', { name: 'Show all trains on this line' })[0]).toHaveAttribute(
      'href',
      `/lines/${ID}#trains`,
    );
    // The picker is a plain GET form with the line's stations.
    const from = screen.getByLabelText<HTMLSelectElement>('From');
    expect(from.form?.getAttribute('method')).toBe('get');
    expect(from.value).toBe('WOK');
    expect([...from.options].map((o) => o.value)).toContain('WEY');
  });

  it('says when the population predates train membership', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(summary({ scopeApplied: false, counts: {} }));
    await render();
    expect(screen.getByText(/every train calling here is listed/)).toBeInTheDocument();
    expect(screen.queryByRole('navigation', { name: 'Direction' })).not.toBeInTheDocument();
  });
});

describe('LineTrainsResults: buses and ferries', () => {
  it('badges a bus and a ferry with the shared ServiceModeBadge, and a train with none', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(
      summary({
        trains: [
          train({ uid: 'C30818', serviceMode: 'bus', liveTracking: false, lineDue: { time: '14:10', dayOffset: 0 } }),
          train({ uid: 'S00002', serviceMode: 'ferry', liveTracking: false, lineDue: { time: '14:20', dayOffset: 0 } }),
          train({ uid: 'C12345', serviceMode: 'train', liveTracking: true, lineDue: { time: '14:30', dayOffset: 0 } }),
        ],
      }),
    );
    const { container } = renderWithMantine(
      await LineTrainsResults({ id: ID, date: DATE, now: NOW, params: parseLinePageParams({}) }),
    );
    const bus = container.querySelector('[data-uid="C30818"] a') as HTMLElement;
    expect(within(bus).getByText('Bus service')).toBeInTheDocument();
    expect(bus.querySelector('[data-service-mode="bus"]')).not.toBeNull();
    expect(visibleText(bus)).toMatch(/Timetable only/);
    const ferry = container.querySelector('[data-uid="S00002"] a') as HTMLElement;
    expect(within(ferry).getByText('Ferry')).toBeInTheDocument();
    const rail = container.querySelector('[data-uid="C12345"] a') as HTMLElement;
    expect(rail.querySelector('[data-service-mode]')).toBeNull();
    expect(visibleText(rail)).not.toMatch(/Timetable only/);
  });

  it('names a ferry with no resolvable destination by its own mode', async () => {
    vi.mocked(api.getLineTrainsSummary).mockResolvedValue(
      summary({
        trains: [
          train({
            uid: 'S00001',
            serviceMode: 'ferry',
            liveTracking: false,
            destination: null,
            lineDue: { time: '14:10', dayOffset: 0 },
          }),
        ],
      }),
    );
    const { container } = renderWithMantine(
      await LineTrainsResults({ id: ID, date: DATE, now: NOW, params: parseLinePageParams({}) }),
    );
    const row = container.querySelector('[data-uid="S00001"] a') as HTMLElement;
    expect(visibleText(row)).toMatch(/Ferry S00001/);
  });
});
