import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { visibleText } from '@/test/routeText';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { LineTimetablePage as TimetablePage, LineTimetableTrain } from '@/lib/types';
import { STATIONS, train } from '@/test/lineTrainsFixtures';
import LineTimetablePage from './page';

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return { ...actual, getLineStatus: vi.fn(), getLineTimetable: vi.fn() };
});

const ID = 'swr-south-west-main';
// 09:00Z on 7 October is 10:00 BST.
const NOW = new Date('2026-10-07T09:00:00Z');

function row(uid: string, time: string, overrides: Partial<LineTimetableTrain> = {}): LineTimetableTrain {
  return {
    ...train({ uid, lineDue: { time, dayOffset: 0 } }),
    time: { time, dayOffset: 0 },
    arrival: null,
    ...overrides,
  };
}

function page(overrides: Partial<TimetablePage> = {}): TimetablePage {
  return {
    lineId: ID,
    date: '2026-10-07',
    scopeApplied: true,
    scopes: ['line'],
    directions: null,
    from: null,
    to: null,
    at: null,
    stations: STATIONS,
    counts: { line: { down: 40, up: 38 }, shared: { down: 9 } },
    trains: [row('A1', '06:00'), row('A2', '06:15')],
    nextCursor: null,
    ...overrides,
  };
}

async function render(searchParams: Record<string, string> = {}) {
  return renderWithMantine(
    await LineTimetablePage({
      params: Promise.resolve({ id: ID }),
      searchParams: Promise.resolve(searchParams),
    }),
  );
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(NOW);
  vi.mocked(api.getLineStatus).mockReset();
  vi.mocked(api.getLineStatus).mockResolvedValue([
    { name: 'South West Main Line' } as Awaited<ReturnType<typeof api.getLineStatus>>[number],
  ]);
  vi.mocked(api.getLineTimetable).mockReset();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('LineTimetablePage', () => {
  it('asks for today’s line trains from the top by default, and lists them by time, each row a link', async () => {
    vi.mocked(api.getLineTimetable).mockResolvedValue(page());
    await render();
    expect(api.getLineTimetable).toHaveBeenCalledWith(ID, {
      date: '2026-10-07',
      dir: null,
      from: null,
      to: null,
      at: null,
      scope: 'line',
      limit: 50,
      after: null,
    });
    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Timetable: South West Main Line');
    expect(screen.getByRole('link', { name: '← Back to South West Main Line' })).toHaveAttribute(
      'href',
      `/lines/${ID}`,
    );
    const list = screen.getByRole('list', { name: 'Trains' });
    const links = within(list).getAllByRole('link');
    expect(links.map((a) => visibleText(a))).toEqual([
      expect.stringMatching(/^06:00 Weymouth Scheduled/),
      expect.stringMatching(/^06:15 Weymouth Scheduled/),
    ]);
    expect(links[0]).toHaveAttribute('href', '/train/A1/2026-10-07');
    expect(screen.getByText('78 trains that day.')).toBeInTheDocument();
  });

  it('passes the URL filters through, drops malformed ones, and shows the arrival at the picked station', async () => {
    vi.mocked(api.getLineTimetable).mockResolvedValue(
      page({
        from: 'WOK',
        to: 'WEY',
        trains: [row('A1', '06:25', { arrival: { time: '08:30', dayOffset: 0 } })],
      }),
    );
    await render({ date: '2026-10-06', dir: 'down', from: 'wok', to: 'WEY', at: '06:00', scope: 'line', after: 'x' });
    expect(api.getLineTimetable).toHaveBeenCalledWith(ID, {
      date: '2026-10-06',
      dir: 'down',
      from: 'WOK',
      to: 'WEY',
      at: '06:00',
      scope: 'line',
      limit: 50,
      after: null,
    });
    expect(screen.getByText(/Times are departures from Woking/)).toBeInTheDocument();
    const list = screen.getByRole('list', { name: 'Trains' });
    expect(visibleText(within(list).getByRole('link'))).toMatch(/^06:25 Weymouth · arr 08:30 Scheduled/);
    expect(within(list).getByRole('link')).toHaveAccessibleName(/arriving at 08:30/);
    expect(screen.getByRole('heading', { level: 2 })).toHaveTextContent(
      'This line’s trains, from Woking to Weymouth, towards Weymouth from 06:00',
    );
    expect(screen.getByRole('link', { name: 'Show the whole day' })).toHaveAttribute(
      'href',
      `/lines/${ID}/timetable?date=2026-10-06&dir=down&from=WOK&to=WEY`,
    );

    vi.mocked(api.getLineTimetable).mockClear();
    await render({ date: '2026-02-30', dir: 'north', from: 'WOKING', at: '7', scope: 'all' });
    expect(api.getLineTimetable).toHaveBeenCalledWith(ID, {
      date: '2026-10-07',
      dir: null,
      from: null,
      to: null,
      at: null,
      scope: 'line',
      limit: 50,
      after: null,
    });
  });

  it('has a GET filter form keeping the direction, with the dates on offer', async () => {
    vi.mocked(api.getLineTimetable).mockResolvedValue(page());
    const { container } = await render({ dir: 'up', from: 'WOK', scope: 'shared' });
    const form = container.querySelector('form') as HTMLFormElement;
    expect(form).toHaveAttribute('action', `/lines/${ID}/timetable`);
    expect(form).toHaveAttribute('method', 'get');
    expect(form.querySelector('input[name="dir"]')).toHaveValue('up');
    expect(within(form).getByLabelText('From')).toHaveValue('WOK');
    expect(within(form).getByLabelText('Trains')).toHaveValue('shared');
    const dates = within(within(form).getByLabelText('Date'))
      .getAllByRole('option')
      .map((o) => o.getAttribute('value'));
    expect(dates).toEqual(['2026-10-04', '2026-10-05', '2026-10-06', '2026-10-07', '2026-10-08']);
    expect(within(form).getByLabelText('Date')).toHaveValue('2026-10-07');
  });

  it('has direction chips with the whole day’s counts for the chosen trains, the current one marked', async () => {
    vi.mocked(api.getLineTimetable).mockResolvedValue(
      page({ counts: { line: { down: 4 }, shared: { down: 2, up: 1 } } }),
    );
    await render({ scope: 'shared', dir: 'up' });
    const nav = screen.getByRole('navigation', { name: 'Direction' });
    const tabs = within(nav).getAllByRole('link');
    expect(tabs.map((a) => visibleText(a))).toEqual(['All 3', 'Towards London Waterloo 1', 'Towards Weymouth 2']);
    expect(tabs[1]).toHaveAttribute('aria-current', 'page');
    expect(tabs[2]).toHaveAttribute('href', `/lines/${ID}/timetable?dir=down&scope=shared`);
  });

  it('badges a replacement bus', async () => {
    vi.mocked(api.getLineTimetable).mockResolvedValue(
      page({ trains: [row('B1', '07:00', { serviceMode: 'replacementBus', liveTracking: false })] }),
    );
    await render();
    const list = screen.getByRole('list', { name: 'Trains' });
    expect(within(list).getByText(/bus/i)).toBeInTheDocument();
  });

  it('loads the next pages in place with the same filters, and says when the day is done', async () => {
    vi.mocked(api.getLineTimetable).mockResolvedValue(page({ nextCursor: '375.A2' }));
    const fetchMock = vi
      .fn()
      .mockResolvedValue(
        new Response(JSON.stringify(page({ trains: [row('A3', '06:30')], nextCursor: null })), { status: 200 }),
      );
    vi.stubGlobal('fetch', fetchMock);
    await render({ dir: 'down' });
    // Without JavaScript: a link to the next page.
    expect(document.querySelector('noscript')).not.toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    });
    expect(fetchMock).toHaveBeenCalledWith(
      `/api/lines/${ID}/timetable?date=2026-10-07&scope=line&dir=down&after=375.A2&limit=50`,
      expect.anything(),
    );
    await waitFor(() => expect(screen.getByRole('list', { name: 'More trains' })).toBeInTheDocument());
    expect(visibleText(within(screen.getByRole('list', { name: 'More trains' })).getByRole('link'))).toMatch(/^06:30/);
    expect(screen.getByText('1 more train loaded.')).toBeInTheDocument();
    expect(screen.getByText("You've reached the end — no more trains on this line that day.")).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('keeps the cursor for a retry when a page fails', async () => {
    vi.mocked(api.getLineTimetable).mockResolvedValue(page({ nextCursor: '375.A2' }));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('boom', { status: 503 })));
    await render();
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    });
    await waitFor(() => expect(screen.getByText("Couldn't load more results. Try again.")).toBeInTheDocument());
    expect(screen.getByRole('button', { name: 'Load more' })).toBeInTheDocument();
  });

  it('says when nothing matches, when nothing is published, and when the API is down', async () => {
    vi.mocked(api.getLineTimetable).mockResolvedValue(page({ trains: [] }));
    await render({ from: 'WOK' });
    expect(screen.getByText(/No trains match these filters/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Clear the filters' })).toHaveAttribute('href', `/lines/${ID}/timetable`);

    vi.mocked(api.getLineTimetable).mockRejectedValue(new ApiNotFoundError('nope'));
    await render({ date: '2026-10-08' });
    expect(screen.getByText('No timetable is published for this line on 8 Oct 2026.')).toBeInTheDocument();

    vi.mocked(api.getLineTimetable).mockRejectedValue(new Error('ECONNREFUSED'));
    await render();
    expect(screen.getByText(/This timetable isn’t available right now/)).toBeInTheDocument();
  });

  it('rejects a malformed line id', async () => {
    await expect(
      LineTimetablePage({ params: Promise.resolve({ id: '../x' }), searchParams: Promise.resolve({}) }),
    ).rejects.toThrow();
    expect(api.getLineTimetable).not.toHaveBeenCalled();
  });
});
