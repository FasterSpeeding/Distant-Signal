import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LINE, NETWORK, TFL_BRANCHED_LINE, TFL_NETWORK, TIMETABLE } from '@/test/transitFixtures';
import * as api from '@/lib/api';
import TransitTimetablePage from './page';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getTransitNetwork: vi.fn(), getTransitLine: vi.fn(), getTransitLineTimetable: vi.fn() };
});

const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
vi.mock('next/navigation', () => ({ notFound: () => notFoundMock() }));

async function render(search: Record<string, string>) {
  const element = await TransitTimetablePage({
    params: Promise.resolve({ network: 'metrolink', line: 'blue' }),
    searchParams: Promise.resolve(search),
  });
  return renderWithMantine(element);
}

describe('/transit/[network]/lines/[line]/timetable', () => {
  beforeEach(() => {
    cleanup();
    vi.mocked(api.getTransitNetwork).mockResolvedValue(NETWORK);
    vi.mocked(api.getTransitLine).mockResolvedValue(LINE);
    vi.mocked(api.getTransitLineTimetable).mockResolvedValue(TIMETABLE);
  });

  it("shows an Underground line's hour, by branch", async () => {
    vi.mocked(api.getTransitNetwork).mockResolvedValue(TFL_NETWORK);
    vi.mocked(api.getTransitLine).mockResolvedValue(TFL_BRANCHED_LINE);
    vi.mocked(api.getTransitLineTimetable).mockResolvedValue({ ...TIMETABLE, branch: 'outbound-2' });
    const element = await TransitTimetablePage({
      params: Promise.resolve({ network: 'tfl', line: 'northern' }),
      searchParams: Promise.resolve({ date: '2026-10-30', from: '08:00', branch: 'outbound-2' }),
    });
    renderWithMantine(element);
    expect(api.getTransitLineTimetable).toHaveBeenLastCalledWith('tfl', 'northern', {
      date: '2026-10-30',
      direction: 'outbound',
      from: '08:00',
      to: '09:00',
      branch: 'outbound-2',
    });
    expect(screen.getByText(/^Morden to High Barnet, /)).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Every branch' }).getAttribute('href')).toBe(
      '/transit/tfl/lines/northern/timetable?date=2026-10-30&direction=outbound&from=08%3A00',
    );
    expect(screen.getByRole('link', { name: 'Morden to Edgware' }).getAttribute('href')).toBe(
      '/transit/tfl/lines/northern/timetable?date=2026-10-30&direction=outbound&from=08%3A00&branch=outbound-1',
    );
    expect(screen.getByRole('link', { name: 'Later' }).getAttribute('href')).toContain(
      'from=09%3A00&branch=outbound-2',
    );
  });

  it('shows a stops-by-trips grid for the window, labelled as not live', async () => {
    await render({ date: '2026-10-14', from: '07:00' });
    expect(api.getTransitLineTimetable).toHaveBeenLastCalledWith('metrolink', 'blue', {
      date: '2026-10-14',
      direction: 'outbound',
      from: '07:00',
      to: '10:00',
    });
    expect(screen.getByRole('heading', { level: 1, name: 'Blue Line timetable' })).toBeTruthy();
    expect(screen.getByText('Scheduled times, not live')).toBeTruthy();
    const eccles = screen.getByRole('rowheader', { name: 'Eccles' }).closest('tr')!;
    expect(eccles.textContent).toContain('07:32');
    expect(eccles.textContent).toContain("Doesn't call");
    expect(screen.getByRole('rowheader', { name: 'Eccles' }).querySelector('a')?.getAttribute('href')).toBe(
      '/transit/metrolink/stops/9400ZZMAECC',
    );
  });

  it('links the other direction and the next and previous windows', async () => {
    await render({ date: '2026-10-14', from: '07:00' });
    expect(screen.getByRole('link', { name: 'Towards Piccadilly' }).getAttribute('href')).toBe(
      '/transit/metrolink/lines/blue/timetable?date=2026-10-14&direction=inbound&from=07%3A00',
    );
    expect(screen.getByRole('link', { name: 'Earlier' }).getAttribute('href')).toContain('from=04%3A00');
    expect(screen.getByRole('link', { name: 'Later' }).getAttribute('href')).toContain('from=10%3A00');
  });

  it('says when there are no departures or no published timetable', async () => {
    vi.mocked(api.getTransitLineTimetable).mockResolvedValue({ ...TIMETABLE, trips: [] });
    await render({ date: '2026-10-14', from: '02:00' });
    expect(screen.getByText('No scheduled departures in this window.')).toBeTruthy();
    cleanup();
    vi.mocked(api.getTransitLineTimetable).mockResolvedValue({ ...TIMETABLE, trips: [], outsideHorizon: true });
    await render({ date: '2027-06-01' });
    expect(screen.getByText("The timetable for this date isn't published yet.")).toBeTruthy();
  });
});
