import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LineTrainsResults } from './LineTrainsResults';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { LineTrainEntry } from '@/lib/types';

vi.mock('@/lib/api');

// Fixed "now" well before every fixture's scheduled time below (all fixture
// times are 06:00 or later London-local) -- every test that doesn't care
// about the upcoming/departed split passes this so its rows land in
// "upcoming" and render directly, matching this suite's pre-existing
// expectations. `2026-09-22T00:00:00Z` is 01:00 BST.
const EARLY_NOW = new Date('2026-09-22T00:00:00Z');

function entry(overrides: Partial<LineTrainEntry> = {}): LineTrainEntry {
  return {
    uid: 'C12345',
    callingPoints: [
      {
        tiploc: 'WATRLMN',
        kind: 'Origin',
        booked_arrival: null,
        booked_departure: '08:00:00',
        is_half_minute_arrival: false,
        is_half_minute_departure: false,
        day_offset: 0,
      },
    ],
    scheduleOriginCrs: null,
    scheduleOriginName: null,
    scheduleDestinationCrs: null,
    scheduleDestinationName: null,
    liveStatus: null,
    ...overrides,
  };
}

describe('LineTrainsResults', () => {
  it('renders the "not available" state on a 404', async () => {
    vi.mocked(api.getLineTrains).mockRejectedValue(new ApiNotFoundError('not found'));
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now: EARLY_NOW }));
    expect(screen.getByText('No scheduled train data is available for this line today.')).toBeInTheDocument();
  });

  it('renders the honest outage state on a non-404 failure', async () => {
    vi.mocked(api.getLineTrains).mockRejectedValue(new Error('connect ECONNREFUSED'));
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now: EARLY_NOW }));
    expect(screen.getByText("Today's trains aren't available right now.")).toBeInTheDocument();
  });

  it('renders the empty state for a real 200 [] response', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now: EARLY_NOW }));
    expect(screen.getByText('No trains are scheduled on this line today.')).toBeInTheDocument();
  });

  it('renders a schedule-only row without a route, with the not-live copy', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([entry()]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now: EARLY_NOW }));
    expect(screen.getByText(/Scheduled — not live yet/)).toBeInTheDocument();
    expect(screen.getByText(/08:00/)).toBeInTheDocument();
  });

  it('renders a live row\'s resolved route and links to /train/{uid}/{date}, with a row-specific accessible name', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({
        liveStatus: {
          trainsId: 1,
          trainId: '1A11',
          originCrs: 'WAT',
          originName: 'London Waterloo',
          destinationCrs: 'ALT',
          destinationName: 'Alton',
          scheduledDeparture: '2026-09-22T08:00:00Z',
          status: 'en_route',
          lastReportedLocation: 'Woking',
          lastEventType: 'DEPARTURE',
          delayMinutes: 4,
          nextCallingPoint: 'ALT',
          etaNext: null,
          etaSource: null,
        },
      }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now: EARLY_NOW }));
    expect(screen.getByText(/London Waterloo \(WAT\) → Alton \(ALT\)/)).toBeInTheDocument();
    expect(screen.getByText(/4m late/)).toBeInTheDocument();
    const link = screen.getByRole('link', {
      name: 'View live status for the 08:00 · London Waterloo (WAT) → Alton (ALT)',
    });
    expect(link).toHaveAttribute('href', '/train/C12345/2026-09-22');
  });

  it('falls back to the schedule-side route when the live record has no schedule match of its own (regression: 2026-09-22 UX review §4.1, "Unknown station")', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({
        scheduleOriginCrs: 'KGX',
        scheduleOriginName: 'London Kings Cross',
        scheduleDestinationCrs: 'YRK',
        scheduleDestinationName: 'York',
        liveStatus: {
          trainsId: 1,
          trainId: '1A11',
          originCrs: null,
          originName: null,
          destinationCrs: null,
          destinationName: null,
          scheduledDeparture: '2026-09-22T08:00:00Z',
          status: 'en_route',
          lastReportedLocation: 'Peterborough',
          lastEventType: 'DEPARTURE',
          delayMinutes: null,
          nextCallingPoint: 'YRK',
          etaNext: null,
          etaSource: null,
        },
      }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'ecml', date: '2026-09-22', now: EARLY_NOW }));
    expect(screen.getByText(/London Kings Cross \(KGX\) → York \(YRK\)/)).toBeInTheDocument();
    expect(screen.queryByText(/Unknown station/)).not.toBeInTheDocument();
  });

  it('falls back to the train UID when neither the live nor the schedule side names a station', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({
        uid: 'NOROUTE1',
        liveStatus: {
          trainsId: 1,
          trainId: '1A11',
          originCrs: null,
          originName: null,
          destinationCrs: null,
          destinationName: null,
          scheduledDeparture: '2026-09-22T08:00:00Z',
          status: 'en_route',
          lastReportedLocation: null,
          lastEventType: null,
          delayMinutes: null,
          nextCallingPoint: null,
          etaNext: null,
          etaSource: null,
        },
      }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'ecml', date: '2026-09-22', now: EARLY_NOW }));
    expect(screen.getByText(/Train NOROUTE1/)).toBeInTheDocument();
    expect(screen.queryByText(/Unknown station/)).not.toBeInTheDocument();
  });

  it('sorts rows by their own scheduled time, not response order', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({ uid: 'LATER', callingPoints: [{ ...entry().callingPoints![0], booked_departure: '10:00:00' }] }),
      entry({ uid: 'EARLIER', callingPoints: [{ ...entry().callingPoints![0], booked_departure: '06:00:00' }] }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now: EARLY_NOW }));
    const links = screen.getAllByRole('link', { name: /View live status/ });
    expect(links[0]).toHaveAttribute('href', '/train/EARLIER/2026-09-22');
    expect(links[1]).toHaveAttribute('href', '/train/LATER/2026-09-22');
  });

  it('renders "Cancelled" for a cancelled service and suppresses the delay figure', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({
        liveStatus: {
          trainsId: 1,
          trainId: '1A11',
          originCrs: 'WAT',
          originName: 'London Waterloo',
          destinationCrs: 'ALT',
          destinationName: 'Alton',
          scheduledDeparture: '2026-09-22T08:00:00Z',
          status: 'cancelled',
          lastReportedLocation: 'Woking',
          lastEventType: 'DEPARTURE',
          delayMinutes: 4,
          nextCallingPoint: 'ALT',
          etaNext: null,
          etaSource: null,
        },
      }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now: EARLY_NOW }));
    expect(screen.getByText(/Cancelled/)).toBeInTheDocument();
    expect(screen.queryByText(/4m late/)).not.toBeInTheDocument();
  });

  it("displays the schedule-side time, not the live service's origin-departure time", async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({
        // The live row's `scheduledDeparture` is the whole service's ORIGIN
        // departure time -- deliberately different here from the
        // schedule-side `booked_departure` at this line's own first calling
        // point, as happens for a train that originates off-line. Under the
        // pre-fix code this would have rendered as "08:30" (Europe/London,
        // BST, formatted via `formatTime`) instead of the schedule-side
        // "09:15" the row's own sort key is based on.
        callingPoints: [{ ...entry().callingPoints![0], booked_departure: '09:15:00' }],
        liveStatus: {
          trainsId: 1,
          trainId: '1A11',
          originCrs: 'WAT',
          originName: 'London Waterloo',
          destinationCrs: 'ALT',
          destinationName: 'Alton',
          scheduledDeparture: '2026-09-22T07:30:00Z',
          status: 'en_route',
          lastReportedLocation: 'Woking',
          lastEventType: 'DEPARTURE',
          delayMinutes: null,
          nextCallingPoint: 'ALT',
          etaNext: null,
          etaSource: null,
        },
      }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now: EARLY_NOW }));
    expect(screen.getByText(/09:15/)).toBeInTheDocument();
    expect(screen.queryByText(/08:30/)).not.toBeInTheDocument();
  });

  it('shows a date/count/timezone caption plus a last-updated line (regression: 2026-09-22 UX review §4.4, no context line at all)', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([entry()]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now: EARLY_NOW }));
    expect(screen.getByText(/1 train scheduled today/)).toBeInTheDocument();
    expect(screen.getByText(/Times in UK local time/)).toBeInTheDocument();
    expect(screen.getByText(/^Updated/)).toBeInTheDocument();
  });

  it('splits departed trains into a collapsed section, upcoming trains shown directly (regression: 2026-09-22 UX review §4.2, past trains first with no cap)', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({ uid: 'DEPARTED', callingPoints: [{ ...entry().callingPoints![0], booked_departure: '06:00:00' }] }),
      entry({ uid: 'UPCOMING', callingPoints: [{ ...entry().callingPoints![0], booked_departure: '20:00:00' }] }),
    ]);
    // 12:00 UTC (13:00 BST) is after the departed train's 06:00 and before
    // the upcoming train's 20:00.
    const now = new Date('2026-09-22T12:00:00Z');
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now }));

    // The upcoming train renders directly (its link is present and
    // accessible without any interaction).
    const links = screen.getAllByRole('link', { name: /View live status/ });
    const hrefs = links.map((link) => link.getAttribute('href'));
    expect(hrefs).toContain('/train/UPCOMING/2026-09-22');
    // The departed train is still in the DOM (inside the collapsed
    // <details>), just not the page's leading content.
    expect(hrefs).toContain('/train/DEPARTED/2026-09-22');
    expect(screen.getByText(/1 earlier train today/)).toBeInTheDocument();
  });

  it('shows the "no more trains today" message when every train has already departed', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([entry()]);
    // Well after the fixture's 08:00 departure.
    const now = new Date('2026-09-22T20:00:00Z');
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22', now }));
    expect(
      screen.getByText('No more trains are scheduled on this line for the rest of today.'),
    ).toBeInTheDocument();
    expect(screen.getByText(/1 earlier train today/)).toBeInTheDocument();
  });
});
